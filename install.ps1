#Requires -Version 5.1
<#
.SYNOPSIS
  Native installer for the peektrace CLI on Windows x64.

.DESCRIPTION
  Downloads the prebuilt standalone peektrace binary from the
  Mark-Life/peektrace GitHub Releases and installs it as peektrace.exe. No
  Node.js, npm, or Bun is required on the target machine.

  Usage:
    irm https://raw.githubusercontent.com/Mark-Life/peektrace/main/install.ps1 | iex

  Optional environment overrides:
    $env:PEEKTRACE_VERSION      Pin a release tag, e.g. cli-v1.2.3 (default: newest cli-v*).
    $env:PEEKTRACE_INSTALL_DIR  Install directory (default: $env:LOCALAPPDATA\peektrace\bin).
    $env:PEEKTRACE_BASE_URL     Release-download base (default: the GitHub releases URL).
    $env:PEEKTRACE_GITHUB_API   Repo API base (default: https://api.github.com/repos/Mark-Life/peektrace).

  The download URL is always composed as: "$BaseUrl/$tag/$asset".
#>

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Force TLS 1.2 for older Windows PowerShell hosts.
try {
  [Net.ServicePointManager]::SecurityProtocol = `
    [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {
  # Newer PowerShell negotiates TLS automatically; ignore if the enum is absent.
}

# --- configuration (env-overridable) ----------------------------------------

$BaseUrl = if ($env:PEEKTRACE_BASE_URL) { $env:PEEKTRACE_BASE_URL } else { 'https://github.com/Mark-Life/peektrace/releases/download' }
$GithubApi = if ($env:PEEKTRACE_GITHUB_API) { $env:PEEKTRACE_GITHUB_API } else { 'https://api.github.com/repos/Mark-Life/peektrace' }
$InstallDir = if ($env:PEEKTRACE_INSTALL_DIR) { $env:PEEKTRACE_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'peektrace\bin' }
$Asset = 'peektrace-windows-x64.exe'
$BinName = 'peektrace.exe'

function Write-Info { param([string]$Message) Write-Host $Message }

function Fail {
  param([string]$Message)
  Write-Error $Message
  exit 1
}

# --- version resolution ------------------------------------------------------

function Resolve-Tag {
  if ($env:PEEKTRACE_VERSION) {
    Write-Info "Using pinned version: $($env:PEEKTRACE_VERSION)"
    return $env:PEEKTRACE_VERSION
  }

  Write-Info 'Resolving latest peektrace CLI release...'
  try {
    $releases = Invoke-RestMethod -Uri "$GithubApi/releases" -Headers @{ 'User-Agent' = 'peektrace-installer' }
  } catch {
    Fail "failed to query GitHub API at $GithubApi/releases : $_"
  }

  # Releases come back newest-first; take the first tag beginning with cli-v.
  $tag = $releases |
    Where-Object { $_.tag_name -like 'cli-v*' } |
    Select-Object -First 1 -ExpandProperty tag_name

  if (-not $tag) {
    Fail "could not find a cli-v* release via $GithubApi/releases"
  }
  Write-Info "Latest version: $tag"
  return $tag
}

# --- checksum verification ---------------------------------------------------

function Test-Checksum {
  param(
    [string]$File,
    [string]$SumsFile
  )

  # SHA256SUMS lines look like: "<hash>  peektrace-windows-x64.exe"
  $expected = $null
  foreach ($line in Get-Content -LiteralPath $SumsFile) {
    $trimmed = $line.Trim()
    if ($trimmed -match "\s$([regex]::Escape($Asset))$") {
      $expected = ($trimmed -split '\s+')[0]
      break
    }
  }
  if (-not $expected) {
    Fail "no SHA256SUMS entry found for $Asset; refusing to install"
  }

  $actual = (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash
  if ($expected.ToLower() -ne $actual.ToLower()) {
    Fail "checksum mismatch for ${Asset}: expected $expected but got $actual. Aborting without installing."
  }
  Write-Info 'Checksum verified.'
}

# --- PATH management ---------------------------------------------------------

function Add-ToUserPath {
  param([string]$Dir)

  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (-not $userPath) { $userPath = '' }

  $parts = $userPath -split ';' | Where-Object { $_ -ne '' }
  if ($parts -contains $Dir) {
    return
  }

  $newPath = if ($userPath -eq '') { $Dir } else { "$userPath;$Dir" }
  [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
  # Update the current session too so the command works immediately.
  $env:Path = "$env:Path;$Dir"

  Write-Info ''
  Write-Info "Added $Dir to your user PATH."
  Write-Info 'Open a new terminal for the change to take effect in other sessions.'
}

# --- main --------------------------------------------------------------------

$tag = Resolve-Tag
$url = "$BaseUrl/$tag/$Asset"
$sumsUrl = "$BaseUrl/$tag/SHA256SUMS"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("peektrace-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

try {
  $binTmp = Join-Path $tmp $Asset
  $sumsTmp = Join-Path $tmp 'SHA256SUMS'

  Write-Info "Downloading $Asset ($tag)..."
  try {
    Invoke-WebRequest -Uri $url -OutFile $binTmp -UseBasicParsing
    Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsTmp -UseBasicParsing
  } catch {
    Fail "download failed: $_"
  }

  Test-Checksum -File $binTmp -SumsFile $sumsTmp

  Write-Info "Installing to $InstallDir\$BinName..."
  New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
  $dest = Join-Path $InstallDir $BinName
  Move-Item -LiteralPath $binTmp -Destination $dest -Force

  Add-ToUserPath -Dir $InstallDir

  Write-Info ''
  Write-Info "Installed peektrace ($tag) -> $dest"
  Write-Info ''
  Write-Info 'Get started:'
  Write-Info '  peektrace serve'
} finally {
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
