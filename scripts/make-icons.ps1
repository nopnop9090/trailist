# Builds the TrayList icon set (a "list" motif on a rounded dark tile).
# Produces PNGs for the window/bundle plus a multi-size .ico for Windows.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$outDir = Join-Path $PSScriptRoot '..\src-tauri\icons'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

function New-Logo([int]$dim) {
  $d = [double]$dim
  $bmp = [System.Drawing.Bitmap]::new($dim, $dim)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.Clear([System.Drawing.Color]::Transparent)

  $inset = $d * 0.06
  $inner = $d - (2.0 * $inset)
  $radius = $d * 0.22
  $arc = $radius * 2.0

  $rect = [System.Drawing.RectangleF]::new([single]$inset, [single]$inset, [single]$inner, [single]$inner)
  $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
  $path.AddArc($rect.X, $rect.Y, [single]$arc, [single]$arc, 180, 90)
  $path.AddArc($rect.Right - [single]$arc, $rect.Y, [single]$arc, [single]$arc, 270, 90)
  $path.AddArc($rect.Right - [single]$arc, $rect.Bottom - [single]$arc, [single]$arc, [single]$arc, 0, 90)
  $path.AddArc($rect.X, $rect.Bottom - [single]$arc, [single]$arc, [single]$arc, 90, 90)
  $path.CloseFigure()

  $c1 = [System.Drawing.Color]::FromArgb(255, 34, 38, 48)
  $c2 = [System.Drawing.Color]::FromArgb(255, 17, 19, 25)
  $tile = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    $rect, $c1, $c2, [System.Drawing.Drawing2D.LinearGradientMode]::Vertical)
  $g.FillPath($tile, $path)

  $accent = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 96, 165, 250))
  $muted = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 148, 163, 184))

  # Three rows: coloured dot plus a text bar, the list metaphor.
  $dot = $d * 0.105
  $barH = $d * 0.075
  $left = $rect.X + ($d * 0.17)
  $firstY = $rect.Y + ($d * 0.24)
  $gap = $d * 0.185
  for ($i = 0; $i -lt 3; $i++) {
    $cy = $firstY + ($i * $gap)
    $brush = if ($i -eq 0) { $accent } else { $muted }
    $g.FillEllipse($brush, [single]$left, [single]$cy, [single]$dot, [single]$dot)

    $barX = $left + $dot + ($d * 0.075)
    $barW = $rect.Right - ($d * 0.17) - $barX
    if ($i -eq 0) { $barW = $barW * 0.72 }
    if ($barW -lt $barH) { $barW = $barH }

    $rr = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $rr.AddArc([single]$barX, [single]$cy, [single]$barH, [single]$barH, 90, 180)
    $rr.AddArc([single]($barX + $barW - $barH), [single]$cy, [single]$barH, [single]$barH, 270, 180)
    $rr.CloseFigure()
    $g.FillPath($brush, $rr)
    $rr.Dispose()
  }

  $accent.Dispose(); $muted.Dispose(); $tile.Dispose(); $path.Dispose(); $g.Dispose()
  return $bmp
}

function Save-Png([int]$dim, [string]$name) {
  $bmp = New-Logo $dim
  $p = Join-Path $outDir $name
  $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output ("wrote " + $p)
}

Save-Png 32 '32x32.png'
Save-Png 128 '128x128.png'
Save-Png 256 '128x128@2x.png'
Save-Png 512 'icon.png'

# icon.ico with PNG-compressed entries (16, 32, 48, 64, 128, 256).
$sizes = @(16, 32, 48, 64, 128, 256)
$pngData = @()
foreach ($s in $sizes) {
  $bmp = New-Logo $s
  $ms = [System.IO.MemoryStream]::new()
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $pngData += , $ms.ToArray()
  $ms.Dispose(); $bmp.Dispose()
}

$icoPath = Join-Path $outDir 'icon.ico'
$fs = [System.IO.File]::Create($icoPath)
$bw = [System.IO.BinaryWriter]::new($fs)
$bw.Write([uint16]0)
$bw.Write([uint16]1)
$bw.Write([uint16]$sizes.Count)
$offset = 6 + (16 * $sizes.Count)
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $s = $sizes[$i]
  $data = $pngData[$i]
  $dimByte = if ($s -ge 256) { 0 } else { $s }
  $bw.Write([byte]$dimByte)
  $bw.Write([byte]$dimByte)
  $bw.Write([byte]0)
  $bw.Write([byte]0)
  $bw.Write([uint16]1)
  $bw.Write([uint16]32)
  $bw.Write([uint32]$data.Length)
  $bw.Write([uint32]$offset)
  $offset += $data.Length
}
foreach ($d in $pngData) { $bw.Write($d) }
$bw.Flush(); $bw.Dispose(); $fs.Dispose()
Write-Output ("wrote " + $icoPath)