$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class QPlayIconHandle
{
    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr icon);
}
'@
$bitmap = [Drawing.Bitmap]::new(256, 256)
$graphics = [Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.Clear([Drawing.Color]::Transparent)
$background = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(23, 27, 33))
$accent = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(0, 157, 247))
$graphics.FillEllipse($background, 8, 8, 240, 240)
$graphics.FillEllipse($accent, 40, 40, 176, 176)
$graphics.FillPolygon([Drawing.Brushes]::White, [Drawing.Point[]]@(
    [Drawing.Point]::new(106, 78),
    [Drawing.Point]::new(178, 128),
    [Drawing.Point]::new(106, 178)
))
$handle = $bitmap.GetHicon()
$icon = [Drawing.Icon]::FromHandle($handle)
$stream = [IO.File]::Create((Join-Path $PSScriptRoot '../assets/qplay.ico'))
try { $icon.Save($stream) }
finally {
    $stream.Dispose()
    $icon.Dispose()
    [void][QPlayIconHandle]::DestroyIcon($handle)
    $graphics.Dispose()
    $bitmap.Dispose()
    $background.Dispose()
    $accent.Dispose()
}
