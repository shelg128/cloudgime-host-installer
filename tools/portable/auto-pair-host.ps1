param(
    [Parameter(Mandatory = $true)]
    [string]$BundleRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$BundleRoot = [System.IO.Path]::GetFullPath($BundleRoot)
$ServerRoot = Join-Path $BundleRoot 'moonlight\server'
$ConfigPath = Join-Path $ServerRoot 'config.json'
$DataPath = Join-Path $ServerRoot 'data.json'
$SecretPath = Join-Path $ServerRoot 'host_provisioning_secret.xml'
$HostInstaller = Join-Path $BundleRoot 'host-installer.exe'
$SunshinePath = Join-Path $BundleRoot 'sunshine\sunshine.exe'
$SunshineDir = Split-Path -Parent $SunshinePath

function Read-JsonFile([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "Required file was not found: $Path"
    }
    return (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json)
}

function Get-ConfigPort([string]$BindAddress) {
    $match = [regex]::Match($BindAddress, ':(\d+)$')
    if (-not $match.Success) {
        throw "Could not determine the local web port from bind_address."
    }
    return [int]$match.Groups[1].Value
}

function New-RandomSecret {
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    return [Convert]::ToBase64String($bytes)
}

function Get-OrCreateProvisioningSecret {
    if (Test-Path -LiteralPath $SecretPath) {
        try {
            $protected = Get-Content -LiteralPath $SecretPath -Raw
            $secure = ConvertTo-SecureString -String $protected
            $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
            try { return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
            finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
        } catch {
            throw 'The local provisioning secret exists but cannot be decrypted by this Windows user.'
        }
    }

    $secret = New-RandomSecret
    $secure = ConvertTo-SecureString -String $secret -AsPlainText -Force
    $protected = ConvertFrom-SecureString -SecureString $secure
    Set-Content -LiteralPath $SecretPath -Value $protected -Encoding ASCII
    return $secret
}

function Invoke-LocalInstaller([string]$CommandName) {
    if (-not (Test-Path -LiteralPath $HostInstaller)) {
        throw 'host-installer.exe was not found in the bundle.'
    }
    & $HostInstaller --bundle-root $BundleRoot $CommandName *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Local host command failed: $CommandName (exit $LASTEXITCODE)."
    }
}

function Wait-Tcp([int]$Port, [int]$TimeoutSeconds = 60) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        try {
            $client = New-Object System.Net.Sockets.TcpClient
            $task = $client.ConnectAsync('127.0.0.1', $Port)
            if ($task.Wait(500) -and $client.Connected) {
                $client.Dispose()
                return
            }
            $client.Dispose()
        } catch { }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "Timed out waiting for local port $Port."
}

function New-HttpClient {
    $handler = New-Object System.Net.Http.HttpClientHandler
    $handler.UseCookies = $false
    $handler.ServerCertificateCustomValidationCallback = { $true }
    return New-Object System.Net.Http.HttpClient($handler)
}

function Invoke-JsonRequest(
    [System.Net.Http.HttpClient]$Client,
    [string]$Method,
    [string]$Url,
    [string]$Cookie,
    [hashtable]$Body,
    [switch]$ResponseHeadersOnly
) {
    $request = New-Object System.Net.Http.HttpRequestMessage([System.Net.Http.HttpMethod]::new($Method), $Url)
    if (-not [string]::IsNullOrWhiteSpace($Cookie)) {
        [void]$request.Headers.TryAddWithoutValidation('Cookie', $Cookie)
    }
    if ($null -ne $Body) {
        $json = $Body | ConvertTo-Json -Depth 20 -Compress
        $request.Content = New-Object System.Net.Http.StringContent($json, [Text.Encoding]::UTF8, 'application/json')
    }
    $completion = if ($ResponseHeadersOnly) {
        [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead
    } else {
        [System.Net.Http.HttpCompletionOption]::ResponseContentRead
    }
    $response = $Client.SendAsync($request, $completion).GetAwaiter().GetResult()
    return $response
}

function Get-Cookie([System.Net.Http.HttpResponseMessage]$Response) {
    $values = @()
    if ($Response.Headers.TryGetValues('Set-Cookie', [ref]$values)) {
        return ($values | Select-Object -First 1).Split(';')[0]
    }
    throw 'Local Cloudgime login did not return a session cookie.'
}

function Test-PairInfo([uint32]$SunshinePort) {
    $root = Read-JsonFile $DataPath
    if ($null -eq $root.hosts) { return $false }
    foreach ($property in $root.hosts.psobject.Properties) {
        $localHost = $property.Value
        $address = [string]$localHost.address
        $port = [int]$localHost.http_port
        if (($address -eq '127.0.0.1' -or $address -eq 'localhost' -or $address -eq '::1') -and $port -eq $SunshinePort) {
            if ($null -ne $localHost.pair_info -and $localHost.pair_info -ne $null) {
                return $true
            }
        }
    }
    return $false
}

function Find-LocalHostId([uint32]$SunshinePort) {
    $root = Read-JsonFile $DataPath
    if ($null -eq $root.hosts) { return $null }
    foreach ($property in $root.hosts.psobject.Properties) {
        $localHost = $property.Value
        $address = [string]$localHost.address
        $port = [int]$localHost.http_port
        if (($address -eq '127.0.0.1' -or $address -eq 'localhost' -or $address -eq '::1') -and $port -eq $SunshinePort) {
            return [uint32]$property.Name
        }
    }
    return $null
}

$config = Read-JsonFile $ConfigPath
$localPort = Get-ConfigPort ([string]$config.web_server.bind_address)
$sunshinePort = [int]$config.moonlight.default_http_port
$prefix = ([string]$config.web_server.url_path_prefix).Trim('/')
$baseUrl = if ([string]::IsNullOrWhiteSpace($prefix)) {
    "http://127.0.0.1:$localPort/"
} else {
    "http://127.0.0.1:$localPort/$prefix/"
}

if (Test-PairInfo $sunshinePort) {
    Write-Output '{"ok":true,"status":"already_paired"}'
    exit 0
}

if (-not (Test-Path -LiteralPath $SunshinePath)) {
    throw 'Sunshine executable was not found for automatic pairing.'
}

$sunshineSecret = Get-OrCreateProvisioningSecret
$cloudgimeSecret = $sunshineSecret
$client = New-HttpClient
try {
    # The universal bundle is intentionally first-run capable. The same local
    # secret is used only for this machine's Cloudgime admin and Sunshine API.
    Invoke-LocalInstaller 'stop-bundle'
    & $SunshinePath --creds admin $sunshineSecret *> $null
    if ($LASTEXITCODE -ne 0) { throw 'Sunshine refused local credential initialization.' }
    Invoke-LocalInstaller 'start-bundle'
    Wait-Tcp $localPort
    Wait-Tcp $sunshinePort
    Wait-Tcp ($sunshinePort + 1)

    $loginResponse = Invoke-JsonRequest $client 'POST' ($baseUrl + 'api/login') '' @{
        name = 'cloudgime-provisioner'
        password = $cloudgimeSecret
    }
    if (-not $loginResponse.IsSuccessStatusCode) {
        throw "Local Cloudgime first-run login failed with HTTP $([int]$loginResponse.StatusCode)."
    }
    $cookie = Get-Cookie $loginResponse
    $loginResponse.Dispose()

    $hostId = Find-LocalHostId $sunshinePort
    if ($null -eq $hostId) {
        $hostResponse = Invoke-JsonRequest $client 'POST' ($baseUrl + 'api/host') $cookie @{
            address = '127.0.0.1'
            http_port = $sunshinePort
        }
        $hostBody = $hostResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        if (-not $hostResponse.IsSuccessStatusCode) {
            throw "Local host registration failed with HTTP $([int]$hostResponse.StatusCode)."
        }
        $hostValue = $hostBody | ConvertFrom-Json
        $hostId = [uint32]$hostValue.host.host_id
        $hostResponse.Dispose()
    }

    $pairResponse = Invoke-JsonRequest $client 'POST' ($baseUrl + 'api/pair') $cookie @{ host_id = $hostId } -ResponseHeadersOnly
    if (-not $pairResponse.IsSuccessStatusCode) {
        throw "Local pairing endpoint failed with HTTP $([int]$pairResponse.StatusCode)."
    }
    $stream = $pairResponse.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
    $reader = New-Object System.IO.StreamReader($stream)
    $paired = $false
    $pinSubmitted = $false
    $sunshineClient = New-HttpClient
    try {
        $sunshineAuth = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes("admin:$sunshineSecret"))
        while ($true) {
            $lineTask = $reader.ReadLineAsync()
            if (-not $lineTask.Wait(10000)) { throw 'Timed out waiting for the local pairing result.' }
            $line = $lineTask.Result
            if ($null -eq $line) { break }
            if ([string]::IsNullOrWhiteSpace($line)) { continue }
            $event = $line | ConvertFrom-Json
            if ($null -ne $event.Pin -and -not $pinSubmitted) {
                $sunshineRequest = New-Object System.Net.Http.HttpRequestMessage([System.Net.Http.HttpMethod]::Post, "https://127.0.0.1:$($sunshinePort + 1)/api/pin")
                $sunshineRequest.Headers.Authorization = New-Object System.Net.Http.Headers.AuthenticationHeaderValue('Basic', $sunshineAuth)
                $pinJson = @{ pin = [string]$event.Pin; name = 'Cloudgime Host Web' } | ConvertTo-Json -Compress
                $sunshineRequest.Content = New-Object System.Net.Http.StringContent($pinJson, [Text.Encoding]::UTF8, 'application/json')
                $sunshineResponse = $sunshineClient.SendAsync($sunshineRequest).GetAwaiter().GetResult()
                $sunshineBody = $sunshineResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                $sunshineResponse.Dispose()
                if (-not $sunshineBody -or -not (($sunshineBody | ConvertFrom-Json).status -eq $true)) {
                    throw 'Sunshine rejected the automatic pairing PIN.'
                }
                $pinSubmitted = $true
            }
            if ($null -ne $event.Paired) { $paired = $true; break }
            if ($null -ne $event.PairError) { throw 'Sunshine/Moonlight automatic pairing returned PairError.' }
        }
    } finally {
        $sunshineClient.Dispose()
        $reader.Dispose()
        $stream.Dispose()
        $pairResponse.Dispose()
    }

    if (-not $paired) {
        throw 'Pairing ended without a verified Paired event.'
    }

    $hostVerification = Invoke-JsonRequest $client 'GET' ("$baseUrl" + "api/host?host_id=$hostId") $cookie $null
    $hostVerificationBody = $hostVerification.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    if (-not $hostVerification.IsSuccessStatusCode) {
        throw "Pairing completed, but host verification returned HTTP $([int]$hostVerification.StatusCode)."
    }
    $hostVerificationValue = $hostVerificationBody | ConvertFrom-Json
    $pairedValue = $hostVerificationValue.host.paired
    $pairedText = if ($pairedValue -is [string]) { [string]$pairedValue } else { [string]$pairedValue }
    if ($pairedText -notmatch '(?i)paired') {
        throw 'Pairing completed, but the local API still reports the host as unpaired.'
    }
    $hostVerification.Dispose()

    if (-not (Test-PairInfo $sunshinePort)) {
        # The API is authoritative; the JSON writer may flush asynchronously.
        Start-Sleep -Milliseconds 500
    }

    $appsResponse = Invoke-JsonRequest $client 'GET' ("$baseUrl" + "api/apps?host_id=$hostId") $cookie $null
    $appsBody = $appsResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    if (-not $appsResponse.IsSuccessStatusCode) {
        throw "Application verification failed with HTTP $([int]$appsResponse.StatusCode)."
    }
    $apps = $appsBody | ConvertFrom-Json
    if ($null -eq $apps.apps -or @($apps.apps).Count -eq 0) {
        throw 'Pairing completed but Sunshine returned no applications.'
    }
    $appsResponse.Dispose()
    Write-Output ('{"ok":true,"status":"paired","hostId":' + $hostId + ',"appsVerified":true}')
    exit 0
} finally {
    $client.Dispose()
}
