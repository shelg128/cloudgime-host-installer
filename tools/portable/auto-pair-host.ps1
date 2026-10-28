param(
    [Parameter(Mandatory = $true)]
    [string]$BundleRoot,
    [string]$FirstBootOperation
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http

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
    $port = [int]$match.Groups[1].Value
    if ($port -lt 1 -or $port -gt 65535) { throw 'Invalid configured web port.' }
    return $port
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
            $protected = (Get-Content -LiteralPath $SecretPath -Raw).Trim()
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
    [IO.File]::WriteAllText($SecretPath, $protected, [Text.Encoding]::ASCII)
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    & icacls.exe $SecretPath /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' ('*'+$sid+':F') | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'LOCAL_PAIRING_SECRET_ACL_FAILED' }
    return $secret
}

function Invoke-LocalInstaller([string]$CommandName) {
    if ($CommandName -eq 'stop-bundle') {
        Stop-Service CloudgimeRuntime-data -Force -ErrorAction SilentlyContinue
        for ($i = 0; $i -lt 30; $i++) {
            if ((Get-Service CloudgimeRuntime-data -ErrorAction SilentlyContinue).Status -eq 'Stopped') { break }
            Start-Sleep -Seconds 1
        }
        return
    }
    if ($CommandName -eq 'start-bundle') {
        Start-Service CloudgimeRuntime-data -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 3
        return
    }
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

function Resolve-SunshineCertificatePath {
    foreach ($p in @(
        (Join-Path $ServerRoot 'sunshine-shared\credentials\cacert.pem'),
        (Join-Path $BundleRoot 'sunshine\config\credentials\cacert.pem'),
        (Join-Path $env:ProgramData 'CloudGime\Sunshine\credentials\cacert.pem')
    )) { if (Test-Path -LiteralPath $p) { return $p } }
    throw 'SUNSHINE_CERTIFICATE_NOT_GENERATED'
}

function New-HttpClient {
    if (-not ('CloudGimeLocalCertPin' -as [type])) {
        Add-Type -ReferencedAssemblies @([System.Net.Http.HttpClient].Assembly.Location) -TypeDefinition @"
using System;
using System.Net.Http;
using System.Net.Security;
using System.Security.Cryptography.X509Certificates;
public static class CloudGimeLocalCertPin {
 public static Func<HttpRequestMessage,X509Certificate2,X509Chain,SslPolicyErrors,bool> Create(string pin, int port) {
  return (request,cert,chain,errors) => request.RequestUri.IsLoopback && request.RequestUri.Scheme == "https" && request.RequestUri.Port == port && cert != null && String.Equals(cert.Thumbprint,pin,StringComparison.OrdinalIgnoreCase);
 }
}
"@
    }
    $pem=[IO.File]::ReadAllText((Resolve-SunshineCertificatePath))
    $raw=$pem.Replace('-----BEGIN CERTIFICATE-----','').Replace('-----END CERTIFICATE-----','')
    $cert=New-Object Security.Cryptography.X509Certificates.X509Certificate2(,[Convert]::FromBase64String($raw))
    $handler = New-Object System.Net.Http.HttpClientHandler
    $handler.UseCookies = $false
    $handler.UseProxy = $false
    $handler.AllowAutoRedirect = $false
    $handler.ServerCertificateCustomValidationCallback = [CloudGimeLocalCertPin]::Create($cert.Thumbprint, ($sunshinePort + 1))
    $cert.Dispose()
    $httpClient = New-Object System.Net.Http.HttpClient($handler)
    $httpClient.Timeout = [TimeSpan]::FromSeconds(45)
    return $httpClient
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
    if($FirstBootOperation -and ([uri]$Url).Scheme-eq'http' -and ([uri]$Url).IsLoopback -and ([uri]$Url).Port-eq$localPort){
        $credentialBytes=[Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes((Join-Path $env:ProgramData 'CloudGime\provisioning\host_app.credential.dpapi')),$null,[Security.Cryptography.DataProtectionScope]::LocalMachine)
        try{$envelope=[Text.Encoding]::UTF8.GetString($credentialBytes)|ConvertFrom-Json;[void]$request.Headers.TryAddWithoutValidation('X-CloudGime-Onboarding',[string]$envelope.deviceToken)}finally{[Array]::Clear($credentialBytes,0,$credentialBytes.Length);$envelope=$null}
    }
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
    [System.Collections.Generic.IEnumerable[string]]$values = $null
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

$broker = Get-Service SentinelNodeBroker -ErrorAction SilentlyContinue
if($FirstBootOperation){
    $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
    if(!$identity.IsSystem-or[Diagnostics.Process]::GetCurrentProcess().SessionId-ne 0){throw 'PAIRING_SYSTEM_REQUIRED'}
    $operation=[guid]::Empty;if(![guid]::TryParseExact($FirstBootOperation,'D',[ref]$operation)-or$operation-eq[guid]::Empty){throw 'PAIRING_OPERATION_INVALID'}
    $p=Join-Path $env:ProgramData 'CloudGime\provisioning';$private=Join-Path $p 'first-boot-shell-private'
    foreach($item in @((Get-Item $private),(Get-Item (Join-Path $private 'handoff.json')))){
        if($item.Attributes-band[IO.FileAttributes]::ReparsePoint){throw 'PAIRING_REPARSE_DENIED'}
        $acl=Get-Acl $item.FullName
        if($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value-notin@('S-1-5-18','S-1-5-32-544')){throw 'PAIRING_UNTRUSTED_OWNER'}
        foreach($rule in $acl.Access){if($rule.AccessControlType-eq'Allow'-and$rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value-notin@('S-1-5-18','S-1-5-32-544')){throw 'PAIRING_PRIVATE_BOUNDARY_REQUIRED'}}
    }
    $h=Read-JsonFile (Join-Path $private 'handoff.json');$m=Read-JsonFile (Join-Path $p 'manifest.json');$s=@($m.components|Where-Object componentType -eq 'sentinel')
    $hPcId = if ($h.Binding.PSObject.Properties.Name -contains 'PcId' -and $h.Binding.PcId) { [string]$h.Binding.PcId } else { [string]$m.binding.pcId }
    $hActId = if ($h.Binding.PSObject.Properties.Name -contains 'ActivationId' -and $h.Binding.ActivationId) { [string]$h.Binding.ActivationId } else { [string]$s[0].applicationActivationId }
    if($h.Binding.OperationId-ne$FirstBootOperation-or$h.Stage-notin@('ARMED','CONFIRMED')-or$s.Count-ne 1-or$hPcId-ne[string]$m.binding.pcId-or$hActId-ne[string]$s[0].applicationActivationId){throw 'PAIRING_FIRST_BOOT_BINDING_CHANGED'}
    $operational=Join-Path $private 'operational\operational.json';if(Test-Path $operational){$o=Read-JsonFile $operational;if($o.Stage-eq'OPERATIONAL'){
        $existingConfig=Read-JsonFile $ConfigPath
        if(Test-PairInfo ([uint32]$existingConfig.moonlight.default_http_port)){Write-Output '{"ok":true,"status":"already_paired","credentialsUnchanged":true}';exit 0}
        throw 'PAIRING_OPERATIONAL_NODE_DENIED'
    }}
    Add-Type -AssemblyName System.Security
    $plain=[Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes((Join-Path $p 'sentinel.credential.dpapi')),$null,[Security.Cryptography.DataProtectionScope]::LocalMachine)
    try{
        $credential=[Text.Encoding]::UTF8.GetString($plain)|ConvertFrom-Json
        $body=@{pcId=[string]$m.binding.pcId;applicationActivationId=$hActId;operationId=$FirstBootOperation;packageSha256=('a'*64)}|ConvertTo-Json -Compress
        $ack=Invoke-RestMethod -Uri 'https://cloudgime.my.id/api/v1/sentinel/managed-upgrade-state' -Method Post -ContentType 'application/json' -Body $body -Headers @{Authorization=('Bearer '+$credential.deviceToken)} -TimeoutSec 15 -MaximumRedirection 0
        if(!$ack.ok-or!$ack.maintenance-or$ack.activeRentals-ne 0-or$ack.cleanupHolds-ne 0-or$ack.activeClaim){throw 'PAIRING_NODE_NOT_IDLE'}
    }finally{[Array]::Clear($plain,0,$plain.Length);$credential=$null}
}
elseif ($broker -and $broker.Status -eq 'Running') { throw 'PAIRING_REQUIRES_STOPPED_BROKER_AND_OPERATOR_MAINTENANCE' }
$config = Read-JsonFile $ConfigPath
$localPort = Get-ConfigPort ([string]$config.web_server.bind_address)
$sunshinePort = [int]$config.moonlight.default_http_port
if ($sunshinePort -lt 1024 -or $sunshinePort -gt 65514) { throw 'Invalid configured Sunshine port.' }
$prefix = ([string]$config.web_server.url_path_prefix).Trim('/')
if ($prefix -and $prefix -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Invalid configured web prefix.' }
$bindUri = [uri]('http://' + [string]$config.web_server.bind_address)
if (-not $bindUri.IsLoopback) { throw 'PAIRING_REQUIRES_LOOPBACK_BINDING' }
$baseUrl = if ([string]::IsNullOrWhiteSpace($prefix)) {
    "http://127.0.0.1:$localPort/"
} else {
    "http://127.0.0.1:$localPort/$prefix/"
}

if (Test-PairInfo $sunshinePort) {
    Write-Output '{"ok":true,"status":"already_paired","credentialsUnchanged":true}'
    exit 0
}

if (-not (Test-Path -LiteralPath $SunshinePath)) {
    throw 'Sunshine executable was not found for automatic pairing.'
}

$sunshineSecret = Get-OrCreateProvisioningSecret
$cloudgimeSecret = $sunshineSecret
$client = $null
try {
    # The universal bundle is intentionally first-run capable. The same local
    # secret is used only for this machine's Cloudgime admin and Sunshine API.
    Invoke-LocalInstaller 'stop-bundle'
    Push-Location $SunshineDir
    try { & $SunshinePath --creds admin $sunshineSecret *> $null; if ($LASTEXITCODE -ne 0) { throw 'Sunshine refused local credential initialization.' } }
    finally { Pop-Location }
    Invoke-LocalInstaller 'start-bundle'
    Wait-Tcp $localPort
    Wait-Tcp $sunshinePort
    Wait-Tcp ($sunshinePort + 1)
    $client = New-HttpClient

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
            if ($event.PSObject.Properties.Name -contains 'Pin' -and -not $pinSubmitted) {
                $sunshineRequest = New-Object System.Net.Http.HttpRequestMessage([System.Net.Http.HttpMethod]::Post, "https://127.0.0.1:$($sunshinePort + 1)/api/pin")
                $sunshineRequest.Headers.Authorization = New-Object System.Net.Http.Headers.AuthenticationHeaderValue('Basic', $sunshineAuth)
                $pinJson = @{ pin = [string]$event.Pin; name = 'Cloudgime Host Web' } | ConvertTo-Json -Compress
                $sunshineRequest.Content = New-Object System.Net.Http.StringContent($pinJson, [Text.Encoding]::UTF8, 'application/json')
                # Host streams the PIN before its asynchronous Moonlight request
                # necessarily reaches Sunshine. A negative PIN response can mean
                # 'no pending client yet'; retry the SAME PIN, never start a second
                # pairing operation or rotate the local credential.
                $sunshineRequest.Dispose()
                $pinDeadline=[datetime]::UtcNow.AddSeconds(15)
                do {
                    $pinRequest=New-Object Net.Http.HttpRequestMessage([Net.Http.HttpMethod]::Post,"https://127.0.0.1:$($sunshinePort + 1)/api/pin")
                    $pinRequest.Headers.Authorization=New-Object Net.Http.Headers.AuthenticationHeaderValue('Basic',$sunshineAuth)
                    $pinRequest.Content=New-Object Net.Http.StringContent($pinJson,[Text.Encoding]::UTF8,'application/json')
                    $pinResponse=$null
                    try {
                        $pinResponse=$sunshineClient.SendAsync($pinRequest).GetAwaiter().GetResult()
                        if([int]$pinResponse.StatusCode-in@(401,403)){throw 'SUNSHINE_PAIRING_AUTH_REJECTED'}
                        $sunshineBody=$pinResponse.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                        if($pinResponse.IsSuccessStatusCode-and$sunshineBody-and(($sunshineBody|ConvertFrom-Json).status-eq$true)){$pinSubmitted=$true}
                    }finally{if($null-ne$pinResponse){$pinResponse.Dispose()};$pinRequest.Dispose()}
                    if(!$pinSubmitted){Start-Sleep -Milliseconds 350}
                }while(!$pinSubmitted-and[datetime]::UtcNow-lt$pinDeadline)
                if(!$pinSubmitted){throw 'SUNSHINE_PAIRING_PENDING_CLIENT_TIMEOUT'}
            }
            if ($event.PSObject.Properties.Name -contains 'Paired') { $paired = $true; break }
            if ($event.PSObject.Properties.Name -contains 'PairError') { throw 'Sunshine/Moonlight automatic pairing returned PairError.' }
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
    $pairedText = [string]$pairedValue
    if (-not [string]::Equals($pairedText, 'Paired', [StringComparison]::OrdinalIgnoreCase)) {
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
$sunshineClient = $null
    if ($null -ne $client) { $client.Dispose() }
    $sunshineSecret = $null; $cloudgimeSecret = $null; $sunshineAuth = $null; $cookie = $null
}
