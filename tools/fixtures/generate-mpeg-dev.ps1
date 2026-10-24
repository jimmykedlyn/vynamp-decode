<#
Dev-only: regenerates the small MPEG audio fixtures in crates/vynamp-decode-mpa/tests/fixtures/
with FFmpeg (libmp3lame and the native mp2 encoder). FFmpeg is never a dependency of the
crates and is never run by the tests.

  . D:\Projects\vynamp-media-player\AMBIENTE.ps1   # sets $env:FFMPEG
  pwsh tools/fixtures/generate-mpeg-dev.ps1 [-FFmpeg <path-to-ffmpeg.exe>]

Input: tools/fixtures/source-sine.wav (48 kHz stereo s32, 4800 frames), looped 30 times so
every fixture is 3 s long (144000 frames at 48 kHz). Any ffmpeg failure aborts. The expected
frame counts recorded in the tests come from `ffprobe -count_packets` on these files.
#>
param([string]$FFmpeg = $env:FFMPEG)
$ErrorActionPreference = 'Stop'
if (-not $FFmpeg) { throw 'ffmpeg path missing: pass -FFmpeg or set $env:FFMPEG' }
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$src = Join-Path $PSScriptRoot 'source-sine.wav'
$out = Join-Path $root 'crates/vynamp-decode-mpa/tests/fixtures'
New-Item -ItemType Directory -Force $out | Out-Null

# name, extra ffmpeg args (before the output file)
$cases = @(
  @('cbr128-48k-stereo.mp3', @('-c:a','libmp3lame','-b:a','128k')),
  @('cbr128-44k-stereo.mp3', @('-c:a','libmp3lame','-b:a','128k','-ar','44100')),
  @('vbr-q2-48k-stereo.mp3', @('-c:a','libmp3lame','-q:a','2')),
  @('mpeg2-22k-stereo.mp3',  @('-c:a','libmp3lame','-b:a','64k','-ar','22050')),
  @('mpeg25-8k-mono.mp3',    @('-c:a','libmp3lame','-b:a','16k','-ar','8000','-ac','1')),
  @('cbr64-44k-mono.mp3',    @('-c:a','libmp3lame','-b:a','64k','-ar','44100','-ac','1')),
  @('nolame-cbr128-48k.mp3', @('-c:a','libmp3lame','-b:a','128k','-write_xing','0')),
  @('mp2-192k-48k-stereo.mp2', @('-c:a','mp2','-b:a','192k')),
  @('mp2-128k-44k-stereo.mp2', @('-c:a','mp2','-b:a','128k','-ar','44100'))
)
foreach ($c in $cases) {
  $name, $extra = $c
  $dst = Join-Path $out $name
  Remove-Item -Force -ErrorAction SilentlyContinue $dst
  & $FFmpeg -v error -y -stream_loop 29 -i $src -fflags +bitexact -flags:a +bitexact @extra $dst
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $name" }
}
Write-Host "wrote $($cases.Count) fixtures to $out"
