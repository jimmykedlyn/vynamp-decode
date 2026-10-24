<#
Dev-only: regenerates the small FLAC fixtures in crates/vynamp-decode-flac/tests/fixtures/
with FFmpeg. FFmpeg is never a dependency of the crates and is never run by the tests.
Task 9 builds the full corpus; these files only exercise frame sync and seek.

  . D:\Projects\vynamp-media-player\AMBIENTE.ps1   # sets $env:FFMPEG
  pwsh tools/fixtures/generate-flac-dev.ps1 [-FFmpeg <path-to-ffmpeg.exe>]

Input: tools/fixtures/source-sine.wav (48 kHz stereo s32, 4800 frames). Any ffmpeg failure aborts.
#>
param([string]$FFmpeg = $env:FFMPEG)
$ErrorActionPreference = 'Stop'
if (-not $FFmpeg) { throw 'ffmpeg path missing: pass -FFmpeg or set $env:FFMPEG' }
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$src = Join-Path $PSScriptRoot 'source-sine.wav'
$out = Join-Path $root 'crates/vynamp-decode-flac/tests/fixtures'
New-Item -ItemType Directory -Force $out | Out-Null

# name, extra ffmpeg args (before the output file)
$cases = @(
  @('sine-16-stereo.flac', @('-sample_fmt','s16','-frame_size','512')),
  @('sine-24-stereo.flac', @('-sample_fmt','s32','-bits_per_raw_sample','24','-frame_size','1152')),
  @('sine-16-mono-tail.flac', @('-ac','1','-sample_fmt','s16','-frame_size','1000'))
)
foreach ($c in $cases) {
  $name, $extra = $c
  $dst = Join-Path $out $name
  Remove-Item -Force -ErrorAction SilentlyContinue $dst
  & $FFmpeg -v error -y -i $src -bitexact -c:a flac @extra $dst
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $name" }
}
Write-Host "wrote $($cases.Count) fixtures to $out"
