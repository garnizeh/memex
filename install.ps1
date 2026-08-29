# Memex Universal Windows Installer (PowerShell)
# Usage: irm https://raw.githubusercontent.com/garnizeh/memex/main/install.ps1 | iex

$ErrorActionPreference = 'Stop'

$Repo = "garnizeh/memex"
$InstallDir = if ($env:MEMEX_INSTALL_DIR) { $env:MEMEX_INSTALL_DIR } else { "$env:LOCALAPPDATA\Programs\memex\bin" }

Write-Host "⚡ Installing Memex on Windows..." -ForegroundColor Cyan

# Resolve latest release
Write-Host "🔍 Resolving latest release from GitHub..." -ForegroundColor Gray
$ReleaseUri = "https://api.github.com/repos/$Repo/releases/latest"
$Release = Invoke-RestMethod -Uri $ReleaseUri -Headers @{ "User-Agent" = "Memex-Installer" }
$Tag = $Release.tag_name

if (-not $Tag) {
    Write-Error "Could not resolve latest release tag from GitHub."
    exit 1
}

$ArtifactName = "memex-windows-x86_64.zip"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Tag/$ArtifactName"

Write-Host "⬇️  Downloading Memex ($Tag) from $DownloadUrl..." -ForegroundColor Cyan

$TempDir = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ([System.Guid]::NewGuid().ToString()))
try {
    $ZipPath = Join-Path $TempDir.FullName $ArtifactName
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipPath -UseBasicParsing

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Expand-Archive -Path $ZipPath -DestinationPath $TempDir.FullName -Force

    $SourceExe = Join-Path $TempDir.FullName "memex.exe"
    $DestExe = Join-Path $InstallDir "memex.exe"
    Copy-Item -Path $SourceExe -Destination $DestExe -Force

    Write-Host "✓ Installed memex binary to $DestExe" -ForegroundColor Green

    # Check and add to User PATH if not present
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        Write-Host "⚙️  Adding $InstallDir to User PATH environment variable..." -ForegroundColor Yellow
        $NewPath = if ($UserPath.EndsWith(";")) { "$UserPath$InstallDir" } else { "$UserPath;$InstallDir" }
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path = "$env:Path;$InstallDir"
    }

    # Auto-register with AI agents
    Write-Host "🤖 Auto-registering Memex with installed AI coding agents..." -ForegroundColor Cyan
    & $DestExe install

    Write-Host "✨ Memex installation complete! Run 'memex --help' to get started." -ForegroundColor Green
}
finally {
    Remove-Item -Recurse -Force $TempDir.FullName -ErrorAction SilentlyContinue
}
