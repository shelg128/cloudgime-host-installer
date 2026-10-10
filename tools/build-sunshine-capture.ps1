param(
    [Parameter(Mandatory=$true)][string]$SourceRoot,
    [Parameter(Mandatory=$true)][string]$ToolchainRoot,
    [Parameter(Mandatory=$true)][string]$BuildRoot,
    [ValidateRange(1,16)][int]$Jobs=4,
    [string]$Cmake='cmake.exe'
)
# Rebuildable shared capture dependency; does not deploy or alter any Host.
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
$oldPath=$env:PATH;$oldPkg=$env:PKG_CONFIG_PATH;$oldBuildVersion=$env:BUILD_VERSION;$oldCommit=$env:COMMIT;$oldBranch=$env:BRANCH
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
    Invoke-NativeChecked $Cmake @('-S',$src,'-B',$build,'-G','Ninja',
        ('-DCMAKE_C_COMPILER='+$tool.Replace('\','/')+'/bin/gcc.exe'),
        ('-DCMAKE_CXX_COMPILER='+$tool.Replace('\','/')+'/bin/g++.exe'),
        '-DCMAKE_BUILD_TYPE=Release','-DBUILD_DOCS=OFF','-DBUILD_TESTS=OFF',
        '-DSUNSHINE_ENABLE_TRAY=ON','-DSUNSHINE_ASSETS_DIR=assets',
        ('-DCMAKE_PREFIX_PATH='+$tool.Replace('\','/')),
        ('-DPKG_CONFIG_EXECUTABLE='+$tool.Replace('\','/')+'/bin/pkgconf.exe'),
        '-DSUNSHINE_PUBLISHER_NAME=CloudGime','-DSUNSHINE_PUBLISHER_WEBSITE=https://cloudgime.my.id',
        '-DSUNSHINE_PUBLISHER_ISSUE_URL=https://cloudgime.my.id','-DCMAKE_POLICY_VERSION_MINIMUM=3.5')
    Invoke-NativeChecked $Cmake @('--build',$build,'--target','sunshine','-j',"$Jobs")
    Invoke-NativeChecked 'node' @((Join-Path $patch 'record-build.mjs'),$src,$build)
} finally {
    $env:PATH=$oldPath;$env:PKG_CONFIG_PATH=$oldPkg;$env:BUILD_VERSION=$oldBuildVersion;$env:COMMIT=$oldCommit;$env:BRANCH=$oldBranch
}
