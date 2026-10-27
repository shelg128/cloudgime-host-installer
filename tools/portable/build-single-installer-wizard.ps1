[CmdletBinding()]
param(
    [string]$PackageRoot = 'C:\Users\Admin\Documents\CLOUDGIME WORKSPACE\02 HASIL BUILD DAN INSTALLER\HEADLESS FRESH 20261015\package',
    [string]$OutputDirectory = '',
    [switch]$SkipHostControlBuild,
    [switch]$SkipBootstrapBuild,
    # Explicit private test lane: permits an unsigned package whose automatic
    # boot has not yet been proven, solely to collect the PC fresh-install and
    # reboot evidence that establishes that proof. Never promotion-eligible.
    [switch]$AllowPrivateTestBuild,
    # Private/self-use builds may be unsigned after automatic boot is proven.
    [switch]$AllowPrivateUnsignedBuild,
    # Legacy name retained for existing local engineering commands.
    [Alias('AllowEngineeringCandidate')]
    [switch]$AllowDiagnosticCandidate
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Require-File([string]$Path, [string]$Description) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Description tidak ditemukan: $Path"
    }
}

function Get-Sha256([string]$Path) {
    # Use .NET directly so detached/noninteractive Windows PowerShell sessions
    # do not depend on the optional Get-FileHash cmdlet being available.
    $stream = [IO.File]::OpenRead($Path)
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        try {
            return ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
        } finally {
            $sha.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

function Test-CandidatePackage([string]$Root) {
    $manifestPath = Join-Path $Root 'headless-node-package.json'
    Require-File $manifestPath 'Manifest package Universal'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.kind -ne 'cloudgime-headless-node-candidate') {
        throw "Jenis package tidak didukung: $($manifest.kind)"
    }
    if ($manifest.PSObject.Properties.Name -notcontains 'automaticBootReady') {
        throw 'Manifest package tidak mendeklarasikan automaticBootReady.'
    }
    if ($manifest.PSObject.Properties.Name -notcontains 'internalUnsigned') {
        throw 'Manifest package tidak mendeklarasikan internalUnsigned.'
    }
    foreach ($entry in @($manifest.files)) {
        $payloadPath = Join-Path $Root $entry.path
        Require-File $payloadPath "Payload terdaftar $($entry.path)"
        if ((Get-Item -LiteralPath $payloadPath).Length -ne [Int64]$entry.bytes) {
            throw "Ukuran payload tidak sesuai: $($entry.path)"
        }
        if ((Get-Sha256 $payloadPath) -ne $entry.sha256.ToLowerInvariant()) {
            throw "Hash payload tidak sesuai: $($entry.path)"
        }
    }
    return $manifest
}

function Update-StagedBundleInventoryFile([string]$BundleRoot, [string]$RelativePath, [string]$SourcePath, [string]$Description) {
    $stagedPath = Join-Path $BundleRoot $RelativePath
    $bundleManifestPath = Join-Path $BundleRoot 'cloudgime-host-bundle.json'
    Require-File $stagedPath "$Description dalam bundle"
    Require-File $bundleManifestPath 'Manifest bundle Universal'
    Require-File $SourcePath "$Description terbaru"

    Copy-Item -LiteralPath $SourcePath -Destination $stagedPath -Force
    $bytes = (Get-Item -LiteralPath $stagedPath).Length
    $sha256 = Get-Sha256 $stagedPath

    $bundleManifest = Get-Content -LiteralPath $bundleManifestPath -Raw | ConvertFrom-Json
    if ($bundleManifest.PSObject.Properties.Name -notcontains 'files') {
        throw 'Manifest bundle tidak mendeklarasikan inventory files.'
    }
    $entries = @($bundleManifest.files | Where-Object { $_.path -ceq $RelativePath })
    if ($entries.Count -ne 1) {
        throw "Inventory $Description bundle harus berisi tepat satu entry."
    }
    $entry = $entries[0]
    if ($entry.PSObject.Properties.Name -notcontains 'size' -or $entry.PSObject.Properties.Name -notcontains 'sha256') {
        throw "Entry $Description bundle tidak memiliki size/hash."
    }
    $entry.size = [Int64]$bytes
    $entry.sha256 = $sha256
    # Windows PowerShell's Set-Content -Encoding utf8 writes a UTF-8 BOM, which
    # JsonDocument.Parse(File.ReadAllBytes(...)) rejects as 0xEF at byte 0.
    [IO.File]::WriteAllText($bundleManifestPath, ($bundleManifest | ConvertTo-Json -Depth 100), [Text.UTF8Encoding]::new($false))
    $manifestBytes = [IO.File]::ReadAllBytes($bundleManifestPath)
    if ($manifestBytes.Length -ge 3 -and $manifestBytes[0] -eq 0xEF -and $manifestBytes[1] -eq 0xBB -and $manifestBytes[2] -eq 0xBF) {
        throw 'Manifest bundle masih memiliki UTF-8 BOM.'
    }

    $verified = Get-Content -LiteralPath $bundleManifestPath -Raw | ConvertFrom-Json
    $verifiedEntry = @($verified.files | Where-Object { $_.path -ceq $RelativePath })
    if ($verifiedEntry.Count -ne 1 -or [Int64]$verifiedEntry[0].size -ne [Int64]$bytes -or $verifiedEntry[0].sha256 -cne $sha256) {
        throw "Manifest $Description bundle gagal disinkronkan."
    }
    return [pscustomobject]@{ Path = "bundle/$RelativePath"; Bytes = [Int64]$bytes; Sha256 = $sha256 }
}

function Update-StagedBundleBootstrap([string]$BundleRoot, [string]$BootstrapPath) {
    # The installed app bootstrap runs the prepare command, while first-boot
    # plans bind the bootstrap copied under bundle/host-control. They must be
    # byte-identical. Otherwise FirstBootInstallerBridge correctly rejects the
    # SYSTEM continuation with FIRST_BOOT_PACKAGE_HASH_CHANGED.
    return Update-StagedBundleInventoryFile $BundleRoot 'host-control/cloudgime-host-bootstrap.exe' $BootstrapPath 'Bootstrap Host Control'
}

function Update-StagedBundleHostInstaller([string]$BundleRoot, [string]$HostInstallerPath) {
    # The bundle is the executable payload run by the SYSTEM first-boot bridge.
    # Never reuse its host installer from an older candidate package.
    return Update-StagedBundleInventoryFile $BundleRoot 'host-installer.exe' $HostInstallerPath 'Host Installer'
}

$portableRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($portableRoot)) {
    throw 'Tidak dapat menentukan direktori skrip build Single Installer.'
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $portableRoot '..\..\release\single-installer-wizard'
}
$repoRoot = (Resolve-Path (Join-Path $portableRoot '..\..')).Path
$tauriRoot = Join-Path $portableRoot 'HostControlApp.Tauri\src-tauri'
$nsisScript = Join-Path $portableRoot 'HostControlApp.Tauri\installer\CloudgimeHostControl.Universal.nsi'
$makensis = 'C:\Program Files (x86)\NSIS\makensis.exe'

Require-File $nsisScript 'Skrip NSIS Universal'
Require-File $makensis 'NSIS makensis'

$packageRoot = (Resolve-Path $PackageRoot).Path
$sourceApp = Join-Path $packageRoot 'app'
$sourceBundle = Join-Path $packageRoot 'bundle'
$packageManifest = Test-CandidatePackage $packageRoot
$automaticBootReady = [bool]$packageManifest.automaticBootReady
$internalUnsigned = [bool]$packageManifest.internalUnsigned
if (-not $automaticBootReady -and -not $AllowPrivateTestBuild -and -not $AllowDiagnosticCandidate) {
    throw 'Package sumber belum automaticBootReady. Untuk uji fresh-install + restart pribadi secara eksplisit gunakan -AllowPrivateTestBuild; untuk kandidat diagnostik gunakan -AllowDiagnosticCandidate.'
}
if ($internalUnsigned -and -not $AllowPrivateTestBuild -and -not $AllowPrivateUnsignedBuild -and -not $AllowDiagnosticCandidate) {
    throw 'Package sumber unsigned. Untuk uji pribadi yang juga belum membuktikan auto boot gunakan -AllowPrivateTestBuild; setelah auto boot terbukti gunakan -AllowPrivateUnsignedBuild.'
}
$selectedPrivateModes = @($AllowPrivateTestBuild, $AllowPrivateUnsignedBuild, $AllowDiagnosticCandidate | Where-Object { [bool]$_ }).Count
if ($selectedPrivateModes -gt 1) {
    throw 'Pilih hanya satu mode: -AllowPrivateTestBuild, -AllowPrivateUnsignedBuild, atau -AllowDiagnosticCandidate.'
}
if ($AllowPrivateUnsignedBuild -and -not $automaticBootReady) {
    throw 'Mode -AllowPrivateUnsignedBuild hanya untuk package yang automaticBootReady. Gunakan -AllowPrivateTestBuild untuk membuktikan automatic boot terlebih dahulu.'
}
if ($AllowPrivateUnsignedBuild -and -not $internalUnsigned) {
    Write-Warning 'Package sudah signed; mode private unsigned tidak diperlukan.'
}
Require-File (Join-Path $sourceApp 'cloudgime-host-bootstrap.exe') 'Bootstrap Universal dari package tervalidasi'
Require-File (Join-Path $sourceBundle 'cloudgime-host-bundle.json') 'Manifest bundle Universal'

if (-not $SkipHostControlBuild) {
    Push-Location $tauriRoot
    try {
        & cargo build --release --message-format=short
        if ($LASTEXITCODE -ne 0) { throw "Build Host Control gagal (exit $LASTEXITCODE)." }
    }
    finally { Pop-Location }
}

$bootstrapProject = Join-Path $portableRoot 'HostControlBootstrap\HostControlBootstrap.csproj'
$bootstrapPublishDirectory = Join-Path $portableRoot 'HostControlBootstrap\bin\Release\net8.0-windows\win-x64\publish'
$bootstrapExecutable = Join-Path $bootstrapPublishDirectory 'cloudgime-host-bootstrap.exe'
$hostInstallerExecutable = Join-Path $repoRoot 'target\release\host_installer.exe'
if (-not $SkipBootstrapBuild) {
    & dotnet publish $bootstrapProject -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:DebugType=None -p:DebugSymbols=false -o $bootstrapPublishDirectory
    if ($LASTEXITCODE -ne 0) { throw "Publish Host Bootstrap gagal (exit $LASTEXITCODE)." }
}
Require-File $bootstrapExecutable 'Bootstrap Universal yang sudah dipublish'
Require-File $hostInstallerExecutable 'Host Installer Universal yang sudah dibuild'

$hostControlCandidates = @(
    (Join-Path $tauriRoot 'target\release\hostcontrolapptauri.exe'),
    (Join-Path $tauriRoot 'target\release\cloudgime-host-control.exe')
)
$hostControl = $hostControlCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $hostControl) {
    throw "Binary Host Control release belum tersedia. Jalankan tanpa -SkipHostControlBuild atau perbaiki cargo build."
}

# NSIS treats spaces inside /DNAME=value arguments poorly. Build from a
# deterministic staging root without spaces, then copy the finished artifact to
# the requested release directory.
$safeBuildRoot = Join-Path $env:TEMP 'cloudgime-single-installer-build'
$stage = Join-Path $safeBuildRoot 'staging'
$stageApp = Join-Path $stage 'app'
$stageBundle = Join-Path $stage 'bundle'
$nsisScriptCopy = Join-Path $safeBuildRoot 'CloudgimeHostControl.Universal.nsi'
$nsisOutputExe = Join-Path $safeBuildRoot 'CloudgimeHostSetup.exe'
$outputExe = Join-Path $OutputDirectory 'CloudgimeHostSetup.exe'

Remove-Item -LiteralPath $safeBuildRoot -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $outputExe -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stageApp, $stageBundle, $OutputDirectory -Force | Out-Null

Copy-Item -Path (Join-Path $sourceApp '*') -Destination $stageApp -Recurse -Force
Copy-Item -Path (Join-Path $sourceBundle '*') -Destination $stageBundle -Recurse -Force
# Never inherit a stale bootstrap from the source package: package the executable
# built from the same source revision as the NSIS workflow. The first-boot
# bridge later verifies its own running bootstrap against the copy in the
# bundle, so update both copies and the bundle inventory atomically in staging.
Copy-Item -LiteralPath $bootstrapExecutable -Destination (Join-Path $stageApp 'cloudgime-host-bootstrap.exe') -Force
$stagedBundleHostInstaller = Update-StagedBundleHostInstaller $stageBundle $hostInstallerExecutable
$stagedBundleBootstrap = Update-StagedBundleBootstrap $stageBundle $bootstrapExecutable
if ((Get-Sha256 (Join-Path $stageApp 'cloudgime-host-bootstrap.exe')) -cne $stagedBundleBootstrap.Sha256) {
    throw 'Bootstrap aplikasi dan bootstrap bundle tidak identik.'
}
# Run the exact bundle preflight against the staged payload before compiling
# NSIS, so an invalid manifest/encoding can never reach a target machine.
$preflightProcess = Start-Process -FilePath (Join-Path $stageApp 'cloudgime-host-bootstrap.exe') -ArgumentList "validate-universal-bundle --silent --bundle-root `"$stageBundle`"" -Wait -PassThru
if ($preflightProcess.ExitCode -ne 0) {
    throw "Preflight bundle staging gagal (exit $($preflightProcess.ExitCode))."
}
Copy-Item -LiteralPath $hostControl -Destination (Join-Path $stageApp 'cloudgime-host-control.exe') -Force
# Copy the .nsi too: makensis mishandles a script path containing spaces when
# it is supplied alongside /DNAME=value directives.
Copy-Item -LiteralPath $nsisScript -Destination $nsisScriptCopy -Force

# The NSIS script owns the output file name through INSTALLER_FILE. Its OutFile
# directive therefore writes directly under OUTPUT_DIR; pass that directory
# instead of a (silently ignored) file path.
# makensis splits /Dname=val at spaces even inside quotes on Windows; convert
# paths to short 8.3 format so space-containing profiles compile deterministically.
$fso = New-Object -ComObject Scripting.FileSystemObject
$stageAppArg = "/DPAYLOAD_APP=" + $fso.GetFolder($stageApp).ShortPath
$stageBundleArg = "/DPAYLOAD_BUNDLE=" + $fso.GetFolder($stageBundle).ShortPath
$safeBuildRootArg = "/DOUTPUT_DIR=" + $fso.GetFolder($safeBuildRoot).ShortPath
$scriptArg = $fso.GetFile($nsisScriptCopy).ShortPath
& $makensis $stageAppArg $stageBundleArg $safeBuildRootArg $scriptArg
if ($LASTEXITCODE -ne 0) { throw "Kompilasi NSIS gagal (exit $LASTEXITCODE)." }
Require-File $nsisOutputExe 'Setup.exe Single Installer Wizard'
Copy-Item -LiteralPath $nsisOutputExe -Destination $outputExe -Force
Require-File $outputExe 'Setup.exe Single Installer Wizard release'

$manifest = [ordered]@{
    schemaVersion = 1
    builtAtUtc = [DateTime]::UtcNow.ToString('o')
    installer = [ordered]@{
        path = 'CloudgimeHostSetup.exe'
        sha256 = Get-Sha256 $outputExe
        bytes = (Get-Item -LiteralPath $outputExe).Length
    }
    payload = [ordered]@{
        bootstrap = [ordered]@{
            path = 'app/cloudgime-host-bootstrap.exe'
            sha256 = Get-Sha256 (Join-Path $stageApp 'cloudgime-host-bootstrap.exe')
            builtFromCurrentSource = $true
        }
        hostControl = [ordered]@{
            path = 'app/cloudgime-host-control.exe'
            sha256 = Get-Sha256 (Join-Path $stageApp 'cloudgime-host-control.exe')
        }
        bundleHostInstaller = [ordered]@{
            path = $stagedBundleHostInstaller.Path
            sha256 = $stagedBundleHostInstaller.Sha256
            bytes = $stagedBundleHostInstaller.Bytes
            builtFromCurrentSource = $true
        }
        bundleBootstrap = [ordered]@{
            path = $stagedBundleBootstrap.Path
            sha256 = $stagedBundleBootstrap.Sha256
            bytes = $stagedBundleBootstrap.Bytes
            matchesAppBootstrap = ((Get-Sha256 (Join-Path $stageApp 'cloudgime-host-bootstrap.exe')) -ceq $stagedBundleBootstrap.Sha256)
        }
        bundleManifest = [ordered]@{
            path = 'bundle/cloudgime-host-bundle.json'
            sha256 = Get-Sha256 (Join-Path $stageBundle 'cloudgime-host-bundle.json')
        }
    }
    acceptance = [ordered]@{
        inlineTokenWizard = $true
        directorySelection = $true
        tokenPassedByCommandLine = $false
        readyVerificationImplemented = $true
        readinessEndpoint = '/api/v1/host-activation/installer-readiness'
        sourcePackageAutomaticBootReady = $automaticBootReady
        sourcePackageInternalUnsigned = $internalUnsigned
        sourcePackageReleaseId = [string]$packageManifest.releaseId
        privateTestBuild = [bool]$AllowPrivateTestBuild
        privateUnsignedBuild = [bool]$AllowPrivateUnsignedBuild
        diagnosticCandidateOverride = [bool]$AllowDiagnosticCandidate
        promotionEligible = ($automaticBootReady -and -not $internalUnsigned -and -not $AllowPrivateTestBuild -and -not $AllowPrivateUnsignedBuild -and -not $AllowDiagnosticCandidate)
    }
}
$manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'single-installer-build-manifest.json') -Encoding utf8

Write-Host "Built: $outputExe"
Write-Host "Manifest: $(Join-Path $OutputDirectory 'single-installer-build-manifest.json')"
if ($AllowPrivateTestBuild) {
    Write-Warning 'Artifact ini khusus pengujian pribadi fresh-install/restart; unsigned dan belum terbukti automatic boot. Jangan dipakai untuk rental atau didistribusikan.'
}
elseif ($AllowPrivateUnsignedBuild) {
    Write-Warning 'Artifact ini khusus pemakaian pribadi dan unsigned; jangan didistribusikan atau dipromosikan sebagai release publik.'
}
elseif ($AllowDiagnosticCandidate) {
    Write-Warning 'Artifact ini kandidat diagnostik dan tidak layak promosi.'
}
