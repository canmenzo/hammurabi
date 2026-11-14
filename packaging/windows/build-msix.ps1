# Builds <Stage>.msix next to the stage folder, which holds the files to install: the package
# for the Microsoft Store. It is not signed: the Store signs what it certifies.
#
# The identity comes from Partner Center (Product management > Product identity), through the
# environment, so that nothing here changes when the product is registered:
#   HAMMURABI_MSIX_NAME            Package/Identity/Name
#   HAMMURABI_MSIX_PUBLISHER       Package/Identity/Publisher, "CN=..."
#   HAMMURABI_MSIX_PUBLISHER_NAME  Package/Properties/PublisherDisplayName
# Without them it is built with placeholders: good for trying it, not for the Store.
param([Parameter(Mandatory)][string]$Stage)
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$match = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1
# A package's version has four parts, and the Store wants the last to be 0.
$version = "$($match.Matches[0].Groups[1].Value).0"
$stageDir = (Resolve-Path $Stage).Path
$out = "$stageDir.msix"

function Value($name, $placeholder) {
    $v = [Environment]::GetEnvironmentVariable($name)
    if ($v) { $v } else { $placeholder }
}
$values = @{
    '@NAME@'           = Value 'HAMMURABI_MSIX_NAME' 'Menzo.Hammurabi'
    '@PUBLISHER@'      = Value 'HAMMURABI_MSIX_PUBLISHER' 'CN=Hammurabi placeholder'
    '@PUBLISHER_NAME@' = Value 'HAMMURABI_MSIX_PUBLISHER_NAME' 'menzo'
    '@VERSION@'        = $version
}

# makeappx comes with the Windows SDK: the newest one there is.
$kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$makeappx = Get-ChildItem $kits -Recurse -Filter makeappx.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -like '*\x64\*' } | Sort-Object FullName | Select-Object -Last 1
if (-not $makeappx) { throw "makeappx.exe not found under ${kits}: install the Windows SDK" }

$layout = Join-Path $env:TEMP "hammurabi-msix-$PID"
Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $layout | Out-Null
try {
    Copy-Item (Join-Path $stageDir '*') $layout -Recurse
    Copy-Item (Join-Path $PSScriptRoot 'msix\*.png') $layout
    $manifest = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'msix\AppxManifest.xml'))
    foreach ($k in $values.Keys) {
        # The values go into XML attributes and text.
        $manifest = $manifest.Replace($k, [Security.SecurityElement]::Escape($values[$k]))
    }
    [IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding($false)))

    Remove-Item $out -ErrorAction SilentlyContinue
    & $makeappx.FullName pack /d $layout /p $out /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }
}
finally {
    Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue
}
"$out ($([int]((Get-Item $out).Length / 1MB)) MB), identity $($values['@NAME@']) $version"
