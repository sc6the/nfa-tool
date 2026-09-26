# Generate the geometric N mark locally; no downloaded image assets.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$iconDir = Join-Path $PSScriptRoot '..\icons'
foreach ($size in @(32, 128, 256)) {
    $bitmap = New-Object System.Drawing.Bitmap($size, $size)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.Clear([System.Drawing.Color]::Black)
    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(238,238,238), ($size * 0.078125))
    $graphics.DrawLine($pen, [single]($size * 0.28), [single]($size * 0.72), [single]($size * 0.28), [single]($size * 0.28))
    $graphics.DrawLine($pen, [single]($size * 0.28), [single]($size * 0.28), [single]($size * 0.72), [single]($size * 0.72))
    $graphics.DrawLine($pen, [single]($size * 0.72), [single]($size * 0.72), [single]($size * 0.72), [single]($size * 0.28))
    $name = if ($size -eq 256) { '128x128@2x.png' } else { "${size}x${size}.png" }
    $bitmap.Save((Join-Path $iconDir $name), [System.Drawing.Imaging.ImageFormat]::Png)
    if ($size -eq 256) { $bitmap.Save((Join-Path $iconDir 'icon.png'), [System.Drawing.Imaging.ImageFormat]::Png) }
    $pen.Dispose(); $graphics.Dispose(); $bitmap.Dispose()
}
$png = [System.IO.File]::ReadAllBytes((Join-Path $iconDir 'icon.png'))
$stream = [System.IO.File]::Create((Join-Path $iconDir 'icon.ico'))
$writer = New-Object System.IO.BinaryWriter($stream)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]1)
    $writer.Write([byte]0); $writer.Write([byte]0); $writer.Write([byte]0); $writer.Write([byte]0)
    $writer.Write([uint16]1); $writer.Write([uint16]32)
    $writer.Write([uint32]$png.Length); $writer.Write([uint32]22); $writer.Write($png)
} finally { $writer.Dispose() }
