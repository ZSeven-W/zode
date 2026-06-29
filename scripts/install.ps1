<#
.SYNOPSIS
  zode installer for Windows — downloads a prebuilt zode.exe from GitHub Releases.

.DESCRIPTION
  Quick install (latest release, including betas):
    irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex

  Pin a version or change the install dir with env vars:
    $env:ZODE_VERSION = 'v0.1.0-beta.1'
    $env:ZODE_BIN_DIR = "$HOME\bin"
    irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex

  Supports Windows x64 and arm64. macOS / Linux: use install.sh.
#>
param(
  [string]$Version = $env:ZODE_VERSION,
  [string]$BinDir  = $env:ZODE_BIN_DIR
)

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$Repo = 'ZSeven-W/zode'
$Bin  = 'zode'

function Info($m) { Write-Host "=> $m" -ForegroundColor Cyan }
function Fail($m) { Write-Host "error: $m" -ForegroundColor Red; exit 1 }

# ---- detect architecture --------------------------------------------------
$archRaw = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $archRaw = $env:PROCESSOR_ARCHITEW6432 }
switch ($archRaw) {
  'ARM64' { $arch = 'arm64' }
  'AMD64' { $arch = 'x64' }
  'x86'   { Fail '32-bit Windows is not supported' }
  default { $arch = 'x64' }
}
$suffix = "$arch-windows"

# ---- resolve version (latest release, including pre-releases) -------------
if (-not $Version) {
  Info 'resolving latest release...'
  try {
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases" `
      -Headers @{ 'User-Agent' = 'zode-installer' }
    $Version = $releases[0].tag_name
  } catch { Fail "could not resolve latest release tag (set `$env:ZODE_VERSION). $_" }
}
$verNum = $Version.TrimStart('v')
$asset  = "$Bin-$verNum-$suffix.zip"
$url    = "https://github.com/$Repo/releases/download/$Version/$asset"

# ---- install dir ----------------------------------------------------------
if (-not $BinDir) { $BinDir = Join-Path $env:LOCALAPPDATA 'Programs\zode' }
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

# ---- download + extract ---------------------------------------------------
Info "installing $Bin $Version ($suffix) -> $BinDir"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("zode-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$zip = Join-Path $tmp $asset
try {
  Invoke-WebRequest -Uri $url -OutFile $zip -Headers @{ 'User-Agent' = 'zode-installer' }
} catch {
  Fail "download failed: $url`n(check that $asset exists for $Version — see https://github.com/$Repo/releases)"
}
Expand-Archive -Path $zip -DestinationPath $tmp -Force
$exe = Join-Path $tmp "$Bin.exe"
if (-not (Test-Path $exe)) { Fail "archive did not contain $Bin.exe" }
Copy-Item -Path $exe -Destination (Join-Path $BinDir "$Bin.exe") -Force
Remove-Item -Recurse -Force $tmp

Info "installed: $(Join-Path $BinDir "$Bin.exe")"

# ---- add to user PATH -----------------------------------------------------
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $BinDir) {
  [Environment]::SetEnvironmentVariable('Path', "$userPath;$BinDir", 'User')
  Info "added $BinDir to your user PATH — open a new terminal to use '$Bin'"
} else {
  Info "done — run '$Bin' to start, or '$Bin --help'"
}
