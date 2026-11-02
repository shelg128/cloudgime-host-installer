param(
    [Parameter(Mandatory=$true)][string]$SourceRoot,
    [Parameter(Mandatory=$true)][string]$ToolchainRoot,
    [Parameter(Mandatory=$true)][string]$BuildRoot,
    [string]$FfmpegBinaries='',
    [string]$NvencBundleInclude='',
    [ValidateRange(1,16)][int]$Jobs=4,
    [string]$Cmake='cmake.exe'
)
# Rebuildable shared capture dependency; does not deploy or alter any Host.
#
# Requires a Sunshine checkout at the revision pinned by upstream-lock.json plus
# its submodules (especially third-party/nanors, third-party/nv-codec-headers,
# third-party/nvapi-open-source-sdk and third-party/tray). Submodules that are
# missing are NOT auto-fetched here: a partial tree produces silent ABI drift.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$patch=Join-Path $repo 'patches\sunshine'
$src=[IO.Path]::GetFullPath($SourceRoot)
$tool=[IO.Path]::GetFullPath($ToolchainRoot)
$build=[IO.Path]::GetFullPath($BuildRoot)
foreach($file in @('bin\gcc.exe','bin\g++.exe','bin\pkgconf.exe')) {
    if(!(Test-Path -LiteralPath (Join-Path $tool $file))){throw "Toolchain missing: $file"}
}
# Missing submodules silently change the ABI; refuse instead of guessing.
foreach($sub in @('third-party\nanors','third-party\nv-codec-headers','third-party\nvapi-open-source-sdk','third-party\tray','third-party\libdisplaydevice','third-party\moonlight-common-c')) {
    $p=Join-Path $src $sub
    if(!(Test-Path -LiteralPath $p)){throw "Submodule missing: $sub (run git submodule update --init --recursive in $src)"}
}
$oldPath=$env:PATH;$oldPkg=$env:PKG_CONFIG_PATH;$oldBuildVersion=$env:BUILD_VERSION;$oldCommit=$env:COMMIT;$oldBranch=$env:BRANCH;$oldFfmpegInc=$env:CG_FFMPEG_INCLUDE
function Invoke-NativeChecked([string]$Exe,[string[]]$Arguments){
    & $Exe @Arguments
    $code=$LASTEXITCODE
    if($code-ne0){throw "$Exe failed ($code)"}
}
try {
    $env:PATH=(Join-Path $tool 'bin')+';'+$oldPath
    $env:PKG_CONFIG_PATH=(Join-Path $tool 'lib\pkgconfig')
    $env:BUILD_VERSION='2025.924.154138-cg-capture1'
    $env:COMMIT='86188d47a7463b0f73b35de18a628353adeaa20e'
    $env:BRANCH='cloudgime/capture-recovery'

    Invoke-NativeChecked 'node' @((Join-Path $patch 'apply-capture-recovery.mjs'),$src)
    Invoke-NativeChecked 'node' @((Join-Path $patch 'apply-toolchain-compat.mjs'),$src)

    # Build fixes required by MSYS2 UCRT64 GCC 16 + the LizardByte build-deps bundle.
    if($NvencBundleInclude){ $env:CG_FFMPEG_INCLUDE=$NvencBundleInclude }
    Invoke-NativeChecked 'node' @((Join-Path $patch 'apply-cg-build-fixes.mjs'),$src)

    $cmakeArgs=@('-S',$src,'-B',$build,'-G','Ninja',
        ('-DCMAKE_C_COMPILER='+$tool.Replace('\','/')+'/bin/gcc.exe'),
        ('-DCMAKE_CXX_COMPILER='+$tool.Replace('\','/')+'/bin/g++.exe'),
        '-DCMAKE_BUILD_TYPE=Release','-DBUILD_DOCS=OFF','-DBUILD_TESTS=OFF',
        '-DSUNSHINE_ENABLE_TRAY=ON','-DSUNSHINE_ASSETS_DIR=assets',
        ('-DCMAKE_PREFIX_PATH='+$tool.Replace('\','/')),
        ('-DPKG_CONFIG_EXECUTABLE='+$tool.Replace('\','/')+'/bin/pkgconf.exe'),
        '-DSUNSHINE_PUBLISHER_NAME=CloudGime','-DSUNSHINE_PUBLISHER_WEBSITE=https://cloudgime.my.id',
        '-DSUNSHINE_PUBLISHER_ISSUE_URL=https://cloudgime.my.id','-DCMAKE_POLICY_VERSION_MINIMUM=3.5')
    if($FfmpegBinaries){
        $fb=[IO.Path]::GetFullPath($FfmpegBinaries)
        if(!(Test-Path -LiteralPath (Join-Path $fb 'lib\libcbs.a'))){throw "FFmpeg bundle missing libcbs.a: $fb"}
        $cmakeArgs+=('-DFFMPEG_PREPARED_BINARIES='+$fb.Replace('\','/'))
    }
    Invoke-NativeChecked $Cmake $cmakeArgs
    Invoke-NativeChecked $Cmake @('--build',$build,'--target','sunshine','-j',"$Jobs")

    # The patch is worthless if it silently dropped out of the binary.
    $exe=Join-Path $build 'sunshine.exe'
    if(!(Test-Path -LiteralPath $exe)){throw "sunshine.exe not produced at $exe"}
    $bytes=[IO.File]::ReadAllBytes($exe)
    $text=[Text.Encoding]::ASCII.GetString($bytes)
    foreach($marker in @('Cloudgime capture-only recovered','encoder retained')){
        if($text.IndexOf($marker) -lt 0){throw "built binary is missing patch marker: $marker"}
    }
    Write-Host ("capture-recovery markers verified; sha256=" + (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash)

    Invoke-NativeChecked 'node' @((Join-Path $patch 'record-build.mjs'),$src,$build)
} finally {
    $env:PATH=$oldPath;$env:PKG_CONFIG_PATH=$oldPkg;$env:BUILD_VERSION=$oldBuildVersion;$env:COMMIT=$oldCommit;$env:BRANCH=$oldBranch
    if($null -eq $oldFfmpegInc){ Remove-Item Env:\CG_FFMPEG_INCLUDE -ErrorAction SilentlyContinue } else { $env:CG_FFMPEG_INCLUDE=$oldFfmpegInc }
}
