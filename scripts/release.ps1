# =============================================================================
# CmdMind — Release Packaging Script (Windows PowerShell)
# =============================================================================
# Builds the Windows release binary, packages zip archive, and produces SHA256SUMS.
# =============================================================================

$ErrorActionPreference = "Stop"

$rootDir = Split-Path -Parent $PSScriptRoot
Set-Location $rootDir

# Extract version from Cargo.toml
$cargoToml = Get-Content (Join-Path $rootDir "Cargo.toml")
$versionLine = $cargoToml | Select-String -Pattern '^version\s*=\s*"([^"]+)"'
if (-not $versionLine) {
    Write-Error "Could not extract version from Cargo.toml."
    exit 1
}
$version = $versionLine.Matches[0].Groups[1].Value

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host " Packaging CmdMind v$version (Windows x86_64)" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

$releaseDir = Join-Path $rootDir "release"
if (-not (Test-Path $releaseDir)) {
    New-Item -ItemType Directory -Path $releaseDir | Out-Null
}

# 1. Build release binary
Write-Host "Building optimized release binary..." -ForegroundColor Yellow
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Error "Release build failed."
    exit $LASTEXITCODE
}

$binPath = Join-Path $rootDir "target\release\cmdmind.exe"
if (-not (Test-Path $binPath)) {
    Write-Error "Binary not found at $binPath"
    exit 1
}

# 2. Stage files for archive
$archiveName = "cmdmind-$version-windows-x86_64.zip"
$archivePath = Join-Path $releaseDir $archiveName
$stageDir = Join-Path $releaseDir "stage_windows_x86_64"

if (Test-Path $stageDir) {
    Remove-Item -Recurse -Force $stageDir
}
New-Item -ItemType Directory -Path (Join-Path $stageDir "zsh") | Out-Null

Copy-Item $binPath -Destination (Join-Path $stageDir "cmdmind.exe")
Copy-Item (Join-Path $rootDir "zsh\cmdmind.zsh") -Destination (Join-Path $stageDir "zsh\cmdmind.zsh")
Copy-Item (Join-Path $rootDir "README.md") -Destination (Join-Path $stageDir "README.md")
Copy-Item (Join-Path $rootDir "LICENSE") -Destination (Join-Path $stageDir "LICENSE")
Copy-Item (Join-Path $rootDir "install.sh") -Destination (Join-Path $stageDir "install.sh")
Copy-Item (Join-Path $rootDir "uninstall.sh") -Destination (Join-Path $stageDir "uninstall.sh")

# 3. Create zip archive
if (Test-Path $archivePath) {
    Remove-Item -Force $archivePath
}
Write-Host "Compressing to $archivePath..." -ForegroundColor Yellow
Compress-Archive -Path "$stageDir\*" -DestinationPath $archivePath -CompressionLevel Optimal

Remove-Item -Recurse -Force $stageDir

# 4. Generate SHA256 checksum
Write-Host "Calculating SHA-256 hash..." -ForegroundColor Yellow
$hash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLower()
$checksumLine = "$hash  $archiveName"

$checksumFile = Join-Path $releaseDir "SHA256SUMS"
Set-Content -Path $checksumFile -Value $checksumLine -Encoding ascii

Write-Host ""
Write-Host "==========================================" -ForegroundColor Green
Write-Host " Windows Release Artifact Generated!" -ForegroundColor Green
Write-Host " Archive:  $archivePath" -ForegroundColor White
Write-Host " Checksum: $hash" -ForegroundColor White
Write-Host "==========================================" -ForegroundColor Green
