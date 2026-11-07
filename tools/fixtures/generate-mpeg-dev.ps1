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

# ---- Layer III decoder fixtures (crates/vynamp-decode-mp3/tests/fixtures) ----
# Short encodes by libmp3lame of synthetic signals with tones, a gliding tone, noise and short
# noise bursts (so that start, short and stop blocks occur), each with the reference decode of
# the same file by FFmpeg's floating point decoder (`mp3float`) as raw interleaved
# little-endian f32. The encodes carry NO Xing/Info/LAME tag (`-write_xing 0`) and the empty
# ID3v2 header that FFmpeg's muxer writes is cut off, so the files start with the first frame.
# With a LAME tag FFmpeg's demuxer asks the decoder to skip
# the encoder delay plus 529 (528 + 1) samples at the start (and the padding at the end), so the
# reference would be trimmed. Without it FFmpeg drops nothing and its output starts with the
# first sample of the first frame, the same alignment as the untrimmed output of the decoder
# under test (offset 0, whole frames).
#   rich: uncorrelated channels   corr: right = 0.9 * left (joint stereo picks MS)   mono
$burst0 = '0.45*(random(2)*2-1)*lt(mod(t,0.06),0.003)'
$burst1 = '0.45*(random(3)*2-1)*lt(mod(t+0.02,0.06),0.003)'
$rich0 = "0.30*sin(2*PI*440*t)+0.15*sin(2*PI*(2000+3000*t)*t)+0.10*(random(0)*2-1)+$burst0"
$rich1 = "0.25*sin(2*PI*660*t)+0.15*sin(2*PI*5300*t)*sin(2*PI*7*t)+0.10*(random(1)*2-1)+$burst1"
$expr = @{
  rich = "${rich0}|${rich1}"
  corr = "0.3*sin(2*PI*440*t)+0.2*sin(2*PI*(1000+2500*t)*t)+0.08*(random(0)*2-1)+$burst0"
  mono = $rich0
}
# name, rate, channels, kind, seconds, encoder args
$l3cases = @(
  @('l3-cbr128-44k-joint',  44100, 2, 'corr', 0.30, @('-b:a','128k')),
  @('l3-cbr320-44k-lr',     44100, 2, 'rich', 0.30, @('-b:a','320k','-joint_stereo','0')),
  @('l3-vbr-v0-44k',        44100, 2, 'rich', 0.30, @('-q:a','0')),
  @('l3-vbr-v2-44k',        44100, 2, 'rich', 0.30, @('-q:a','2')),
  @('l3-vbr-v5-44k',        44100, 2, 'rich', 0.30, @('-q:a','5')),
  @('l3-lr-48k-128k',       48000, 2, 'rich', 0.30, @('-b:a','128k','-joint_stereo','0')),
  @('l3-ms-48k-192k',       48000, 2, 'corr', 0.30, @('-b:a','192k')),
  @('l3-cbr96-32k',         32000, 2, 'rich', 0.30, @('-b:a','96k')),
  @('l3-mono-44k-64k',      44100, 1, 'mono', 0.30, @('-b:a','64k')),
  @('l3-mpeg2-22k-64k',     22050, 2, 'rich', 0.50, @('-b:a','64k')),
  @('l3-mpeg2-24k-48k',     24000, 2, 'corr', 0.50, @('-b:a','48k')),
  @('l3-mpeg2-16k-32k',     16000, 2, 'rich', 0.50, @('-b:a','32k')),
  @('l3-mpeg25-12k-mono',   12000, 1, 'mono', 0.70, @('-b:a','24k')),
  @('l3-mpeg25-11k-stereo', 11025, 2, 'rich', 0.70, @('-b:a','32k')),
  @('l3-mpeg25-8k-mono',     8000, 1, 'mono', 1.00, @('-b:a','16k'))
)
foreach ($c in $l3cases) {
  $name, $rate, $ch, $kind, $secs, $enc = $c
  $mp3 = Join-Path $mp3out "$name.mp3"
  $ref = Join-Path $mp3out "$name.f32"
  Remove-Item -Force -ErrorAction SilentlyContinue $mp3, $ref
  # Commas inside the expression would end the filter option.
  $e = $expr[$kind] -replace ',', '\,'
  $af = @()
  if ($kind -eq 'corr') { $af = @('-af', 'pan=stereo|c0=c0|c1=0.9*c0') }
  & $FFmpeg -v error -y -f lavfi -i "aevalsrc=${e}:s=${rate}:d=${secs}" @af -fflags +bitexact -flags:a +bitexact -c:a libmp3lame @enc -write_xing 0 $mp3
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg encode failed for $name" }
  $b = [IO.File]::ReadAllBytes($mp3)
  if ($b[0] -eq 0x49 -and $b[1] -eq 0x44 -and $b[2] -eq 0x33) {
    $n = 10 + (($b[6] -shl 21) -bor ($b[7] -shl 14) -bor ($b[8] -shl 7) -bor $b[9])
    [IO.File]::WriteAllBytes($mp3, $b[$n..($b.Length - 1)])
  }
  & $FFmpeg -v error -y -c:a mp3float -i $mp3 -f f32le -c:a pcm_f32le $ref
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg decode failed for $name" }
}
Write-Host "wrote $($l3cases.Count) Layer III fixtures to $mp3out"

# ---- Gapless fixtures (crates/vynamp-decode/tests/fixtures/mpeg) ----
# PCM derived from tools/fixtures/source-sine.wav (looped, downmixed or resampled) as 16-bit
# WAV (`<name>.src.wav`), the libmp3lame encode of exactly that WAV WITH FFmpeg's Xing/Info and
# LAME tag (`<name>.mp3`; the tag carries the encoder delay and padding), and FFmpeg's own
# gapless decode of the MP3 by `mp3float` (`<name>.ref.f32`, raw interleaved little-endian f32):
# FFmpeg's demuxer reads the LAME tag and trims the encoder delay + 529 (528 + 1) samples at the start
# and the padding at the end. The facade tests compare the Reader's output against both.
$gout = Join-Path $root 'crates/vynamp-decode/tests/fixtures/mpeg'
New-Item -ItemType Directory -Force $gout | Out-Null
# name, source loops, extra filter/format args (before the WAV), encoder args
$gcases = @(
  @('g-cbr128-48k-stereo', 3, @(), @('-b:a','128k')),
  @('g-vbr2-48k-stereo',   3, @(), @('-q:a','2')),
  @('g-mono-48k-64k',      3, @('-ac','1'), @('-b:a','64k')),
  @('g-mpeg2-22k-stereo',  3, @('-ar','22050'), @('-b:a','64k')),
  @('g-cbr128-48k-odd',    1, @('-af','atrim=end_sample=3999'), @('-b:a','128k'))
)
foreach ($c in $gcases) {
  $name, $loops, $pre, $enc = $c
  $wav = Join-Path $gout "$name.src.wav"
  $mp3 = Join-Path $gout "$name.mp3"
  $ref = Join-Path $gout "$name.ref.f32"
  Remove-Item -Force -ErrorAction SilentlyContinue $wav, $mp3, $ref
  & $FFmpeg -v error -y -stream_loop ($loops - 1) -i $src @pre -fflags +bitexact -c:a pcm_s16le $wav
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg wav failed for $name" }
  & $FFmpeg -v error -y -i $wav -fflags +bitexact -flags:a +bitexact -c:a libmp3lame @enc $mp3
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg encode failed for $name" }
  & $FFmpeg -v error -y -c:a mp3float -i $mp3 -f f32le -c:a pcm_f32le $ref
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg decode failed for $name" }
}
Write-Host "wrote $($gcases.Count) gapless fixtures to $gout"
