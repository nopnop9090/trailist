<#
.SYNOPSIS
  Builds TrayList: the release exe plus an NSIS installer.

.DESCRIPTION
  Follows the same contract as the rest of the machine's tooling. The result is
  portable: the exe reads its settings from a `config` folder next to itself, so
  copying the exe and that folder to another machine carries everything across.

.PARAMETER Portable
  Also assemble dist-app/portable/ with the exe and a starter config beside it.

.PARAMETER SkipInstaller
  Build only the exe, skipping the (slower) NSIS bundle.
#>
param(
  [string]$Root = (Resolve-Path "$PSScriptRoot\..").Path,
  [switch]$Portable,
  [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
Set-Location $Root

# A running instance holds a lock on the exe.
$running = Get-Process -Name "TrayList", "trailist" -ErrorAction SilentlyContinue
if ($running) {
  Write-Host ""
  Write-Host "TrayList laeuft noch (PID $($running.Id -join ', '))." -ForegroundColor Yellow
  Write-Host "Ueber das Tray-Menue beenden und das Skript erneut starten." -ForegroundColor Yellow
  exit 1
}

foreach ($tool in @("node", "npm", "cargo")) {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
    Write-Error "$tool wird gebraucht, ist aber nicht im PATH."
    exit 1
  }
}

if (-not (Test-Path "$Root\node_modules")) {
  Write-Host "==> npm-Abhaengigkeiten installieren" -ForegroundColor Cyan
  npm install
  # Vite braucht esbuilds Plattform-Binary, das npm nicht von selbst baut.
  npm rebuild esbuild
}

Write-Host "==> Icons erzeugen" -ForegroundColor Cyan
& "$Root\scripts\make-icons.ps1"

Write-Host "==> Frontend bauen" -ForegroundColor Cyan
npm run build
if ($LASTEXITCODE -ne 0) { Write-Error "Frontend-Build fehlgeschlagen."; exit 1 }

$targets = if ($SkipInstaller) { @("--no-bundle") } else { @("--bundles", "nsis") }

Write-Host "==> Release bauen (der erste Lauf dauert mehrere Minuten)" -ForegroundColor Cyan
# $targets statt @targets: der Splat-Operator zerlegt native Argumente.
npm run tauri -- build $targets
if ($LASTEXITCODE -ne 0) { Write-Error "Tauri-Build fehlgeschlagen."; exit 1 }

$releaseDir = "$Root\src-tauri\target\release"
$exe = Join-Path $releaseDir "TrayList.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $releaseDir "trailist.exe" }
if (-not (Test-Path $exe)) { Write-Error "Keine exe in $releaseDir gefunden."; exit 1 }

$dist = Join-Path $Root "dist-app"
New-Item -ItemType Directory -Path $dist -Force | Out-Null
Copy-Item $exe (Join-Path $dist "TrayList.exe") -Force
# Auch neben config/ legen, damit das Repo-Root direkt startbar ist.
Copy-Item $exe (Join-Path $Root "TrayList.exe") -Force

Write-Host ""
Write-Host "Exe:       $(Join-Path $Root 'TrayList.exe')" -ForegroundColor Green
Write-Host "           $([math]::Round((Get-Item $exe).Length / 1MB, 1)) MB"

if (-not $SkipInstaller) {
  $installer = Get-ChildItem "$releaseDir\bundle\nsis\*.exe" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if ($installer) {
    Copy-Item $installer.FullName $dist -Force
    Write-Host "Installer: $(Join-Path $dist $installer.Name)" -ForegroundColor Green
  }
}

if ($Portable) {
  $portableDir = Join-Path $dist "portable"
  New-Item -ItemType Directory -Path (Join-Path $portableDir "config") -Force | Out-Null
  Copy-Item (Join-Path $dist "TrayList.exe") $portableDir -Force
  Write-Host "Portable:  $portableDir" -ForegroundColor Green
}