# fastskill installer script for Windows.
#
#   irm https://github.com/gofastskill/fastskill/releases/latest/download/install.ps1 | iex
#   irm https://github.com/gofastskill/fastskill/releases/download/v1.4.2/install.ps1 | iex
#
# Environment (all optional):
#   FASTSKILL_VERSION             stable (default) | latest | 1.4.2
#   FASTSKILL_INSTALL_DIR         bin dir (default: $HOME\.local\bin)
#   FASTSKILL_NO_MODIFY_PATH=1    leave the user PATH untouched
#   FASTSKILL_UNMANAGED=1         no PATH edit, no receipt (images, CI)
#   FASTSKILL_INSTALLER_BASE_URL  https mirror serving the same asset names
#                                   (plain http only for 127.0.0.1, localhost, [::1])
#   FASTSKILL_GITHUB_TOKEN / GITHUB_TOKEN   private GitHub releases (header
#                                   only; never sent to a mirror)
#
# This script only downloads and verifies. Placement, PATH and the install
# receipt are done by `fastskill self install`, so this file rarely changes.
# The body is a function called on the last line: a truncated download runs
# nothing.

function Install-fastskill {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue'
  # Windows PowerShell 5.1 on older Windows 10 builds does not default to TLS 1.2.
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

  $App = 'fastskill'
  $Repo = 'gofastskill/fastskill'
  $TagPrefix = 'v'
  # How the self group is reached: 'self', or 'cli self' when the app puts
  # its built-in commands under a namespace.
  $SelfCmd = @('cli self' -split ' ')
  $Version = if ($env:FASTSKILL_VERSION) { $env:FASTSKILL_VERSION } else { 'stable' }
  $Token = if ($env:FASTSKILL_GITHUB_TOKEN) { $env:FASTSKILL_GITHUB_TOKEN } elseif ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } else { $null }
  $Mirror = $env:FASTSKILL_INSTALLER_BASE_URL
  if ($Mirror) {
    # A mirror never receives the GitHub token.
    $Token = $null
    $uri = [Uri]$Mirror
    if ($uri.Scheme -ne 'https' -and -not ($uri.Scheme -eq 'http' -and $uri.IsLoopback)) {
      throw 'FASTSKILL_INSTALLER_BASE_URL must be https (plain http is allowed only for a loopback address)'
    }
  }
  $Headers = @{}
  if ($Token) { $Headers['Authorization'] = "Bearer $Token" }

  # OSArchitecture needs .NET Framework 4.7.1; older hosts fall back to the
  # environment, where PROCESSOR_ARCHITEW6432 reveals a 64-bit OS under WOW64.
  $osArch = try { [string][System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture } catch { $null }
  if (-not $osArch) {
    $osArch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
  }
  $arch = switch ($osArch) {
    'Arm64' { 'aarch64' }
    'X64'   { 'x86_64' }
    'AMD64' { 'x86_64' }
    default { throw "unsupported architecture: $osArch" }
  }
  $target = "$arch-pc-windows-msvc"
  $asset = "$App-$target.zip"

  # Same rule as the binary: XDG_BIN_HOME is not consulted on Windows.
  $binDir = if ($env:FASTSKILL_INSTALL_DIR) { $env:FASTSKILL_INSTALL_DIR }
            else { Join-Path $HOME '.local\bin' }
  New-Item -ItemType Directory -Force -Path $binDir | Out-Null
  # Temp dir inside the bin dir: same volume for the final move.
  $tmp = Join-Path $binDir (".$App-install-" + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $tmp | Out-Null

  try {
    $api = "https://api.github.com/repos/$Repo/releases"
    if ($Mirror) {
      $base = $Mirror.TrimEnd('/')
      if ($Version -in @('stable', 'latest')) {
        $v = (Invoke-RestMethod -UseBasicParsing "$base/latest.json").version
        if (-not $v) { throw "could not read version from $base/latest.json" }
        $base = "$base/$TagPrefix$v"
      } else { $base = "$base/$TagPrefix$($Version.TrimStart('v'))" }
      $archiveUrl = "$base/$asset"; $sumsUrl = "$base/SHA256SUMS"
      $dlHeaders = @{}
    } elseif ($Token) {
      # Private repo: asset downloads go through the API by asset id.
      $rel = switch ($Version) {
        'stable' { Invoke-RestMethod -UseBasicParsing -Headers $Headers "$api/latest" }
        'latest' { @(Invoke-RestMethod -UseBasicParsing -Headers $Headers "$($api)?per_page=1")[0] }
        default  { Invoke-RestMethod -UseBasicParsing -Headers $Headers "$api/tags/$TagPrefix$($Version.TrimStart('v'))" }
      }
      $a = $rel.assets | Where-Object name -eq $asset | Select-Object -First 1
      $s = $rel.assets | Where-Object name -eq 'SHA256SUMS' | Select-Object -First 1
      if (-not $a -or -not $s) { throw "release assets not found for $asset" }
      $archiveUrl = $a.url; $sumsUrl = $s.url
      $dlHeaders = $Headers + @{ 'Accept' = 'application/octet-stream' }
    } else {
      $base = switch ($Version) {
        'stable' { "https://github.com/$Repo/releases/latest/download" }
        'latest' {
          $tag = @(Invoke-RestMethod -UseBasicParsing "$($api)?per_page=1")[0].tag_name
          if (-not $tag) { throw 'could not resolve the latest release tag' }
          "https://github.com/$Repo/releases/download/$tag"
        }
        default { "https://github.com/$Repo/releases/download/$TagPrefix$($Version.TrimStart('v'))" }
      }
      $archiveUrl = "$base/$asset"; $sumsUrl = "$base/SHA256SUMS"
      $dlHeaders = @{}
    }

    $zip = Join-Path $tmp $asset
    $sums = Join-Path $tmp 'SHA256SUMS'
    Write-Host "downloading $asset ($Version)"
    Invoke-WebRequest -UseBasicParsing -Headers $dlHeaders -Uri $archiveUrl -OutFile $zip
    Invoke-WebRequest -UseBasicParsing -Headers $dlHeaders -Uri $sumsUrl -OutFile $sums

    $line = Get-Content $sums | Where-Object { $_ -match "^\s*([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($asset))\s*$" } | Select-Object -First 1
    if (-not $line) { throw "$asset is not listed in SHA256SUMS" }
    $expected = ($line -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
    if ($expected -ne $actual) { throw "checksum mismatch for $asset" }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $exe = Join-Path $tmp "$App.exe"
    if (-not (Test-Path $exe)) { throw "archive does not contain $App.exe" }
    # Invoke-WebRequest attaches the mark-of-the-web; drop it before running.
    Unblock-File -Path $exe

    $cliArgs = $SelfCmd + @('install', '--from-bootstrap', '--bin-dir', $binDir)
    if ($env:FASTSKILL_NO_MODIFY_PATH) { $cliArgs += '--no-modify-path' }
    if ($env:FASTSKILL_UNMANAGED) { $cliArgs += '--unmanaged' }

    # Bootstrap mode moves the binary into place; finally removes the
    # now-empty temp dir.
    & $exe @cliArgs
    if ($LASTEXITCODE -ne 0) { throw "$App self install exited with code $LASTEXITCODE" }
  }
  finally {
    Remove-Item -Recurse -Force -Path $tmp -ErrorAction SilentlyContinue
  }
}

Install-fastskill
