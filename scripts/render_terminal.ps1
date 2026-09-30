# Rasterize the terminal framebuffer SVG with Windows' installed monospace font.
Add-Type -AssemblyName System.Drawing
$projectRoot = Split-Path -Parent $PSScriptRoot
[xml]$svg = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'docs/images/terminal.svg')
$bitmap = [System.Drawing.Bitmap]::new([int]$svg.svg.width, [int]$svg.svg.height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.Clear([System.Drawing.ColorTranslator]::FromHtml('#0f141e'))
foreach ($rect in $svg.svg.rect) {
    if ($rect.width -eq '100%') { continue }
    $brush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($rect.fill))
    $graphics.FillRectangle($brush, [single]$rect.x, [single]$rect.y, [single]$rect.width, [single]$rect.height)
    $brush.Dispose()
}
$font = [System.Drawing.Font]::new('Consolas', 16, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
$graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
foreach ($item in $svg.svg.g.text) {
    $brush = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml($item.fill))
    $graphics.DrawString($item.InnerText, $font, $brush, [single]$item.x, ([single]$item.y - 15), [System.Drawing.StringFormat]::GenericTypographic)
    $brush.Dispose()
}
$bitmap.Save((Join-Path $projectRoot 'docs/images/terminal.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$font.Dispose()
$graphics.Dispose()
$bitmap.Dispose()
