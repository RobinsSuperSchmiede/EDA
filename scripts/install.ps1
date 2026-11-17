# OpenShell installer for Windows.
#
#   irm https://raw.githubusercontent.com/RobinsSuperSchmiede/EDA/main/scripts/install.ps1 | iex
#
# Installs openshell.exe and openshell-desktop.exe from the latest GitHub release into
# %LOCALAPPDATA%\OpenShell\bin and adds that folder to the user PATH.
# Set $env:OPENSHELL_VERSION = "v0.1.0" to pin a version.

$ErrorActionPreference = "Stop"
$repo = "RobinsSuperSchmiede/EDA"
$target = "x86_64-pc-windows-msvc"
$binDir = Join-Path $env:LOCALAPPDATA "OpenShell\bin"

if ($env:OPENSHELL_VERSION) {
    $url = "https://github.com/$repo/releases/download/$($env:OPENSHELL_VERSION)/openshell-$target.zip"
} else {
    $url = "https://github.com/$repo/releases/latest/download/openshell-$target.zip"
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("openshell-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp "openshell.zip") -UseBasicParsing
    Expand-Archive -Path (Join-Path $tmp "openshell.zip") -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force $binDir | Out-Null
    Copy-Item (Join-Path $tmp "openshell-$target\openshell.exe") $binDir -Force
    Copy-Item (Join-Path $tmp "openshell-$target\openshell-desktop.exe") $binDir -Force
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not ($userPath -split ";" | Where-Object { $_ -eq $binDir })) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$binDir", "User")
    Write-Host "Added $binDir to your user PATH (open a new terminal)."
}

Write-Host ""
Write-Host "OpenShell installed to $binDir"
Write-Host "Next: openshell create my-app; cd my-app; openshell dev"
