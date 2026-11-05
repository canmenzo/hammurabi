# Builds <Stage>-setup.exe next to the stage folder, which holds the files to install.
# Inno Setup is downloaded on first use, and the pinned hash has to match.
param(
    [Parameter(Mandatory)][string]$Stage,
    [string]$InnoDir = (Join-Path $env:TEMP 'hammurabi-inno-setup-6.7.3')
)
$ErrorActionPreference = 'Stop'

$url = 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe'
$sha256 = '9C73C3BAE7ED48D44112A0F48E66742C00090BDB5BEF71D9D3C056C66E97B732'
$iscc = Join-Path $InnoDir 'ISCC.exe'

if (-not (Test-Path $iscc)) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $download = Join-Path $env:TEMP 'hammurabi-innosetup.exe'
    Invoke-WebRequest -UseBasicParsing -UserAgent 'OpenAI File Downloader, XaiImageApiFetch/1.0' -Uri $url -OutFile $download
    if ((Get-FileHash $download -Algorithm SHA256).Hash -ne $sha256) { throw 'Inno Setup download does not match its pinned hash' }
    $args = '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/PORTABLE=1', '/CURRENTUSER', "/DIR=`"$InnoDir`""
    $p = Start-Process $download -ArgumentList $args -Wait -PassThru
    if ($p.ExitCode -ne 0 -or -not (Test-Path $iscc)) { throw "Inno Setup install failed ($($p.ExitCode))" }
    Remove-Item $download
}

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$match = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1
$version = $match.Matches[0].Groups[1].Value
$stageDir = (Resolve-Path $Stage).Path
$name = Split-Path $stageDir -Leaf

& $iscc /Q "/DAppVersion=$version" "/DStage=$stageDir" "/O$(Split-Path $stageDir -Parent)" "/F$name-setup" (Join-Path $PSScriptRoot 'hammurabi.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE)" }
