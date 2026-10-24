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
  @('cbr32-48k-stereo.mp3',  @('-c:a','libmp3lame','-b:a','32k')),
  @('cbr8-8k-mono.mp3',      @('-c:a','libmp3lame','-b:a','8k','-ar','8000','-ac','1')),
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

# ---- Layer II decoder fixtures (crates/vynamp-decode-mp3/tests/fixtures) ----
# Short (0.2 s) encodes of a synthetic broadband signal (tones, a gliding tone and noise,
# different in the two channels), each with the reference decode of the same file by FFmpeg's
# floating point Layer II decoder (`mp2float`) as raw interleaved little-endian f32. The
# decoder tests compare against the .f32 file. The joint stereo and CRC fixtures need
# libtwolame (the native mp2 encoder has neither); the others use the native encoder.
$mp3out = Join-Path $root 'crates/vynamp-decode-mp3/tests/fixtures'
New-Item -ItemType Directory -Force $mp3out | Out-Null
$e0 = '0.30*sin(2*PI*440*t)+0.15*sin(2*PI*(2000+3000*t)*t)+0.10*(random(0)*2-1)'
$e1 = '0.25*sin(2*PI*660*t)+0.15*sin(2*PI*5300*t)*sin(2*PI*7*t)+0.10*(random(1)*2-1)'
# name, rate, channels, encoder args
$l2cases = @(
  @('l2-stereo-48k-192k',  48000, 2, @('-c:a','mp2','-b:a','192k')),
  @('l2-joint-44k-128k',   44100, 2, @('-c:a','libtwolame','-mode','joint_stereo','-error_protection','1','-b:a','128k')),
  @('l2-mono-48k-64k',     48000, 1, @('-c:a','mp2','-b:a','64k')),
  @('l2-lsf-24k-64k',      24000, 2, @('-c:a','mp2','-b:a','64k'))
)
foreach ($c in $l2cases) {
  $name, $rate, $ch, $enc = $c
  $mp2 = Join-Path $mp3out "$name.mp2"
  $ref = Join-Path $mp3out "$name.f32"
  Remove-Item -Force -ErrorAction SilentlyContinue $mp2, $ref
  $expr = if ($ch -eq 2) { "$e0|$e1" } else { $e0 }
  & $FFmpeg -v error -y -f lavfi -i "aevalsrc=${expr}:s=${rate}:d=0.2" -fflags +bitexact -flags:a +bitexact @enc $mp2
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg encode failed for $name" }
  & $FFmpeg -v error -y -c:a mp2float -i $mp2 -f f32le -c:a pcm_f32le $ref
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg decode failed for $name" }
}
Write-Host "wrote $($l2cases.Count) Layer II fixtures to $mp3out"
