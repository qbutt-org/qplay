$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../assets/qplay.svg'
$icon = Join-Path $PSScriptRoot '../assets/qplay.ico'

& magick -background none $source -define icon:auto-resize=256,64,48,32,24,20,16 $icon
if ($LASTEXITCODE -ne 0) {
    throw 'Could not generate the Windows icon.'
}

Copy-Item -LiteralPath $source -Destination (Join-Path $PSScriptRoot '../docs/assets/icon.svg')
