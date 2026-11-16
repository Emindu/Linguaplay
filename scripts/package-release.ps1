<#
  Builds a portable Windows release of Linguaplay and zips it.

  Output: release-dist/Linguaplay-<version>-windows-x64.zip containing
    Linguaplay.exe, libvlc.dll, libvlccore.dll and VLC's plugins/ folder
  (libVLC finds plugins/ beside its own DLL, so the zip runs without VLC
  being installed). yt-dlp and ffmpeg are still needed on PATH for YouTube.

  Needs a 64-bit VLC install to copy libVLC from (default location, or set
  VLC_DIR), the same one the normal build uses.
#>
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Get-Content package.json -Raw | ConvertFrom-Json).version
$vlc = if ($env:VLC_DIR) { $env:VLC_DIR } else { "C:\Program Files\VideoLAN\VLC" }
if (-not (Test-Path "$vlc\libvlc.dll")) { throw "libvlc.dll not found in '$vlc'. Install 64-bit VLC or set VLC_DIR." }

# --no-bundle: only the exe is wanted here, not the MSI/NSIS installers.
npm run tauri build -- --no-bundle
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

$name = "Linguaplay-$version-windows-x64"
$stage = Join-Path $root "release-dist\$name"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item "src-tauri\target\release\linguaplay.exe" "$stage\Linguaplay.exe"
Copy-Item "$vlc\libvlc.dll", "$vlc\libvlccore.dll" $stage
Copy-Item "$vlc\plugins" "$stage\plugins" -Recurse
Copy-Item "$vlc\COPYING.txt" "$stage\VLC-COPYING.txt" -ErrorAction SilentlyContinue
Copy-Item README.md $stage

$zip = Join-Path $root "release-dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path "$stage\*" -DestinationPath $zip -CompressionLevel Optimal
"{0}  ({1:N1} MB)" -f $zip, ((Get-Item $zip).Length / 1MB)
