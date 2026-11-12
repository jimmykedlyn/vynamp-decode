<#
Dev-only: regenerates tests/fixtures/pcm/* and tests/fixtures/flac/* (with MANIFEST.txt) with FFmpeg.
FFmpeg is never a dependency of the crates and is never run by the tests.

  $env:FFMPEG = 'D:\Projects\vynamp\.cache\tools\ffmpeg\bin\ffmpeg.exe'   # see engineering/08-reinicio-windows.md (umbrella)
  pwsh tools/fixtures/generate.ps1 [-FFmpeg <path-to-ffmpeg.exe>]

Input: tools/fixtures/source-sine.wav (48 kHz stereo s32, 4800 frames, made by
`cargo run -p vynamp-decode-testkit --example gen_source_sine`). Any ffmpeg failure aborts.
#>
param([string]$FFmpeg = $env:FFMPEG)
$ErrorActionPreference = 'Stop'
if (-not $FFmpeg) { throw 'ffmpeg path missing: pass -FFmpeg or set $env:FFMPEG' }
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$src = Join-Path $PSScriptRoot 'source-sine.wav'
$out = Join-Path $root 'tests/fixtures/pcm'
New-Item -ItemType Directory -Force $out | Out-Null
$gen = ((& $FFmpeg -version)[0]).Trim()
if ($LASTEXITCODE -ne 0) { throw 'ffmpeg -version failed' }

$six = 'pan=6c|c0=c0|c1=c1|c2=c0|c3=c1|c4=c0|c5=c1'
# name, channels, spec, extra ffmpeg args (before the output file)
$cases = @(
  @('sine-u8.wav',        2, 'u8',    @('-c:a','pcm_u8')),
  @('sine-s16le.wav',     2, 's16le', @('-c:a','pcm_s16le')),
  @('sine-s24le.wav',     2, 's24le', @('-c:a','pcm_s24le')),
  @('sine-s32le.wav',     2, 's32le', @('-c:a','pcm_s32le')),
  @('sine-f32le.wav',     2, 'f32le', @('-c:a','pcm_f32le')),
  @('sine-f64le.wav',     2, 'f64le', @('-c:a','pcm_f64le')),
  @('sine-6ch-s24le.wav', 6, 's24le', @('-af',$six,'-c:a','pcm_s24le')),
  @('sine-s16le-rf64.wav',2, 's16le', @('-c:a','pcm_s16le','-rf64','always')),
  @('sine-s16le.w64',     2, 's16le', @('-c:a','pcm_s16le','-f','w64')),
  @('sine-s16be.aif',     2, 's16be', @('-c:a','pcm_s16be','-f','aiff')),
  @('sine-s24be.aif',     2, 's24be', @('-c:a','pcm_s24be','-f','aiff')),
  @('sine-sowt.aifc',     2, 's16le', @('-c:a','pcm_s16le','-f','aiff')),
  @('sine-fl32.aifc',     2, 'f32be', @('-c:a','pcm_f32be','-f','aiff'))
)
$rows = @('# arquivo|sha256|rate|channels|spec|frames|gerador')
foreach ($c in $cases) {
  $name, $ch, $spec, $extra = $c
  $dst = Join-Path $out $name
  Remove-Item -Force -ErrorAction SilentlyContinue $dst
  & $FFmpeg -v error -y -i $src -bitexact @extra $dst
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $name" }
  $sha = (Get-FileHash -Algorithm SHA256 $dst).Hash.ToLower()
  $rows += "$name|$sha|48000|$ch|$spec|4800|$gen"
}
[IO.File]::WriteAllText((Join-Path $out 'MANIFEST.txt'), (($rows -join "`n") + "`n"))
Write-Host "wrote $($cases.Count) fixtures to $out"

# ---------------------------------------------------------------------------------
# FLAC oracle corpus -> tests/fixtures/flac/ (+ ref/ with the PCM FFmpeg fed to the encoder)
#
# FLAC is lossless, so the decoded stream must equal the PCM the encoder received. Every
# FLAC file is derived from source-sine.wav through an FFmpeg filter chain; the same chain
# writes a reference WAV (pcm_s16le/s24le/s32le), which our already-proven PCM path reads.
# Each FLAC is also decoded with FFmpeg here and compared with its reference (any mismatch
# aborts), proving the reference is what the encoder consumed.
# The pinned FFmpeg cannot write 8-bit FLAC (the encoder accepts s16/s32 only; with
# -bits_per_raw_sample 8 it still writes bps 16); 8-bit stays hand-assembled/conformance.
# 32-bit needs -strict experimental. FFmpeg's FLAC muxer writes no SEEKTABLE and no
# ID3v2: the tests add those synthetically.
# ---------------------------------------------------------------------------------
$fout = Join-Path $root 'tests/fixtures/flac'
$fref = Join-Path $fout 'ref'
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $fout
New-Item -ItemType Directory -Force $fref | Out-Null
[IO.File]::WriteAllText((Join-Path $fout '.gitattributes'), "* -text`n")

$mono = 'pan=mono|c0=c0'
$wasted = 'aformat=sample_fmts=s16,aformat=sample_fmts=s32'
# ref name -> (pcm codec, filter/rate args)
$refs = [ordered]@{
  'st16-48k'      = @('pcm_s16le', @())
  'st24-48k'      = @('pcm_s24le', @())
  'st32-48k'      = @('pcm_s32le', @())
  'mono16-48k'    = @('pcm_s16le', @('-af', $mono))
  'ch6-24-48k'    = @('pcm_s24le', @('-af', $six))
  'st24-96k'      = @('pcm_s24le', @('-ar', '96000'))
  'st16-192k'     = @('pcm_s16le', @('-ar', '192000'))
  'st16-44k1'     = @('pcm_s16le', @('-ar', '44100'))
  'st24w-48k'     = @('pcm_s24le', @('-af', $wasted))
}
$rows = @('# kind|arquivo|sha256|ref|args|gerador')
foreach ($k in $refs.Keys) {
  $codec, $pre = $refs[$k]
  $dst = Join-Path $fref "$k.wav"
  & $FFmpeg -v error -y -i $src -bitexact @pre -c:a $codec $dst
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for ref $k" }
  $sha = (Get-FileHash -Algorithm SHA256 $dst).Hash.ToLower()
  $rows += "ref|ref/$k.wav|$sha|-|$($pre -join ' ') -c:a $codec|$gen"
}

$s16 = @('-sample_fmt', 's16')
$s24 = @('-sample_fmt', 's32', '-bits_per_raw_sample', '24')
$s32 = @('-strict', 'experimental', '-sample_fmt', 's32', '-bits_per_raw_sample', '32')
# name, ref, args before the output (filters/rate come from the ref's definition)
$flacs = @(
  @('st16-level0.flac',   'st16-48k',   ($s16 + @('-compression_level', '0'))),
  @('st16-level5.flac',   'st16-48k',   ($s16 + @('-compression_level', '5'))),
  @('st16-level8.flac',   'st16-48k',   ($s16 + @('-compression_level', '8'))),
  @('st16-level12.flac',  'st16-48k',   ($s16 + @('-compression_level', '12'))),
  @('st16-fixed.flac',    'st16-48k',   ($s16 + @('-lpc_type', 'fixed'))),
  @('st16-levinson.flac', 'st16-48k',   ($s16 + @('-lpc_type', 'levinson'))),
  @('st16-cholesky.flac', 'st16-48k',   ($s16 + @('-lpc_type', 'cholesky'))),
  @('st16-fs192.flac',    'st16-48k',   ($s16 + @('-frame_size', '192'))),
  @('st16-fs4096.flac',   'st16-48k',   ($s16 + @('-frame_size', '4096'))),
  @('st16-fs16384.flac',  'st16-48k',   ($s16 + @('-frame_size', '16384'))),
  @('st16-tags.flac',     'st16-48k',   ($s16 + @('-metadata', 'title=Vynamp oracle', '-metadata', 'artist=Test'))),
  @('st16-midside.flac',  'st16-48k',   ($s16 + @('-ch_mode', 'mid_side'))),
  @('st16-leftside.flac', 'st16-48k',   ($s16 + @('-ch_mode', 'left_side'))),
  @('st16-rightside.flac','st16-48k',   ($s16 + @('-ch_mode', 'right_side'))),
  @('st24.flac',          'st24-48k',   $s24),
  @('st32.flac',          'st32-48k',   $s32),
  @('st24-wasted.flac',   'st24w-48k',  $s24),
  @('mono16.flac',        'mono16-48k', $s16),
  @('ch6-24.flac',        'ch6-24-48k', $s24),
  @('st24-96k.flac',      'st24-96k',   $s24),
  @('st16-192k.flac',     'st16-192k',  $s16),
  @('st16-44k1.flac',     'st16-44k1',  $s16)
)
function Hash-Pcm([string]$file) {
  $h = & $FFmpeg -v error -i $file -c:a pcm_s32le -f hash -hash sha256 -
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg hash failed for $file" }
  return ($h -join '').Trim()
}
foreach ($c in $flacs) {
  $name, $ref, $extra = $c
  $codec, $pre = $refs[$ref]
  $dst = Join-Path $fout $name
  & $FFmpeg -v error -y -i $src -bitexact @pre -c:a flac @extra $dst
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $name" }
  $refFile = Join-Path $fref "$ref.wav"
  if ((Hash-Pcm $dst) -ne (Hash-Pcm $refFile)) { throw "${name}: FFmpeg's decode differs from reference $ref" }
  $sha = (Get-FileHash -Algorithm SHA256 $dst).Hash.ToLower()
  $rows += "flac|$name|$sha|ref/$ref.wav|$($pre -join ' ') $($extra -join ' ')|$gen"
}
[IO.File]::WriteAllText((Join-Path $fout 'MANIFEST.txt'), (($rows -join "`n") + "`n"))
Write-Host "wrote $($refs.Count) references and $($flacs.Count) FLAC files to $fout"

# ---------------------------------------------------------------------------------
# MPEG audio oracle corpus -> tests/fixtures/mpeg/ (MP2 and MP3 files + FFmpeg's f32 decode)
#
# Every file is a short encode of a synthetic signal made here with `aevalsrc` (tones, a gliding
# tone, noise, short noise bursts so that all Layer III block types occur; right = 0.9 * left for
# the `corr` kind so that joint stereo picks mid/side). Nothing is read from outside the
# repository. Layer II comes from FFmpeg's native `mp2` encoder (libtwolame for joint stereo and
# CRC), Layer III from libmp3lame (CBR, VBR -q:a, ABR, mono, joint stereo, MPEG-2 and MPEG-2.5).
# Each file is decoded by FFmpeg's floating point decoders (`mp2float`, `mp3float`) into
# `<name>.f32` (raw interleaved little-endian f32), the oracle of oracle_mpeg.rs. Files carry no
# Xing/Info/LAME tag and no ID3v2 header, so FFmpeg drops nothing and its output starts with the
# first sample of the first frame; the Reader trims the decoder delay (481 for Layer II, 529
# for Layer III). Layer I has no encoder in FFmpeg: no Layer I fixtures (the layer stays
# experimental). The Layer II column `table` is the allocation table the standard selects for
# the file's rate and bitrate per channel (A/B 27/30 subbands, C/D low rate 8/12 subbands,
# LSF = the MPEG-2 table); oracle_mpeg.rs re-derives it from each file's headers and compares.
# ---------------------------------------------------------------------------------
$mout = Join-Path $root 'tests/fixtures/mpeg'
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $mout
New-Item -ItemType Directory -Force $mout | Out-Null
[IO.File]::WriteAllText((Join-Path $mout '.gitattributes'), "* -text`n")

$b0 = '0.45*(random(2)*2-1)*lt(mod(t,0.06),0.003)'
$b1 = '0.45*(random(3)*2-1)*lt(mod(t+0.02,0.06),0.003)'
$r0 = "0.30*sin(2*PI*440*t)+0.15*sin(2*PI*(2000+3000*t)*t)+0.10*(random(0)*2-1)+$b0"
$r1 = "0.25*sin(2*PI*660*t)+0.15*sin(2*PI*5300*t)*sin(2*PI*7*t)+0.10*(random(1)*2-1)+$b1"
$mexpr = @{
  rich = "${r0}|${r1}"
  corr = "0.3*sin(2*PI*440*t)+0.2*sin(2*PI*(1000+2500*t)*t)+0.08*(random(0)*2-1)+$b0"
  mono = $r0
}
function Write-MpegCase([string]$name, [string]$ext, [int]$rate, [int]$ch, [string]$kind, [double]$secs, [string[]]$enc, [string]$dec) {
  $mp = Join-Path $mout "$name.$ext"
  $ref = Join-Path $mout "$name.f32"
  $e = $mexpr[$kind] -replace ',', '\,'
  $af = @()
  if ($kind -eq 'corr') { $af = @('-af', 'pan=stereo|c0=c0|c1=0.9*c0') }
  & $FFmpeg -v error -y -f lavfi -i "aevalsrc=${e}:s=${rate}:d=${secs}" @af -fflags +bitexact -flags:a +bitexact @enc $mp
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg encode failed for $name" }
  $b = [IO.File]::ReadAllBytes($mp)
  if ($b[0] -eq 0x49 -and $b[1] -eq 0x44 -and $b[2] -eq 0x33) {
    $n = 10 + (($b[6] -shl 21) -bor ($b[7] -shl 14) -bor ($b[8] -shl 7) -bor $b[9])
    [IO.File]::WriteAllBytes($mp, $b[$n..($b.Length - 1)])
  }
  & $FFmpeg -v error -y -c:a $dec -i $mp -f f32le -c:a pcm_f32le $ref
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg decode failed for $name" }
  $sha = (Get-FileHash -Algorithm SHA256 $mp).Hash.ToLower()
  $rsha = (Get-FileHash -Algorithm SHA256 $ref).Hash.ToLower()
  return "$name.$ext|$sha|$name.f32|$rsha|$rate|$ch|$kind ${secs}s"
}
$mrows = @('# arquivo|sha256|ref|sha256 ref|rate|channels|sinal|layer|nota|args|gerador')
# name, rate, channels, kind, seconds, note (Layer II: allocation table), encoder args
$l2 = @(
  @('l2-A-48k-st-128k',    48000, 2, 'rich', 0.35, 'A',   @('-c:a','mp2','-b:a','128k')),
  @('l2-A-44k-mono-64k',   44100, 1, 'mono', 0.35, 'A',   @('-c:a','mp2','-b:a','64k')),
  @('l2-B-44k-st-384k',    44100, 2, 'rich', 0.35, 'B',   @('-c:a','mp2','-b:a','384k')),
  @('l2-B-32k-st-256k',    32000, 2, 'rich', 0.35, 'B',   @('-c:a','mp2','-b:a','256k')),
  @('l2-C-48k-st-96k',     48000, 2, 'rich', 0.35, 'C',   @('-c:a','mp2','-b:a','96k')),
  @('l2-C-48k-st-64k',     48000, 2, 'rich', 0.35, 'C',   @('-c:a','mp2','-b:a','64k')),
  @('l2-C-48k-joint-96k',  48000, 2, 'corr', 0.35, 'C',   @('-c:a','libtwolame','-mode','joint_stereo','-error_protection','1','-b:a','96k')),
  @('l2-D-32k-st-64k',     32000, 2, 'rich', 0.35, 'D',   @('-c:a','mp2','-b:a','64k')),
  @('l2-D-32k-mono-32k',   32000, 1, 'mono', 0.35, 'D',   @('-c:a','mp2','-b:a','32k')),
  @('l2-LSF-24k-st-64k',   24000, 2, 'rich', 0.35, 'LSF', @('-c:a','mp2','-b:a','64k')),
  @('l2-LSF-22k-st-96k',   22050, 2, 'rich', 0.35, 'LSF', @('-c:a','mp2','-b:a','96k')),
  @('l2-LSF-16k-mono-32k', 16000, 1, 'mono', 0.35, 'LSF', @('-c:a','mp2','-b:a','32k'))
)
foreach ($c in $l2) {
  $name, $rate, $ch, $kind, $secs, $note, $enc = $c
  $row = Write-MpegCase $name 'mp2' $rate $ch $kind $secs $enc 'mp2float'
  $mrows += "$row|2|table $note|$($enc -join ' ')|$gen"
}
# name, rate, channels, kind, seconds, mode, encoder args (libmp3lame; -write_xing 0 is added)
$l3 = @(
  @('l3-cbr032-44k-st',   44100, 2, 'rich', 0.25, 'CBR 32',          @('-b:a','32k')),
  @('l3-cbr096-44k-st',   44100, 2, 'rich', 0.25, 'CBR 96',          @('-b:a','96k')),
  @('l3-cbr192-44k-st',   44100, 2, 'rich', 0.25, 'CBR 192',         @('-b:a','192k')),
  @('l3-cbr256-48k-st',   48000, 2, 'rich', 0.25, 'CBR 256',         @('-b:a','256k')),
  @('l3-cbr320-44k-lr',   44100, 2, 'rich', 0.25, 'CBR 320 LR',      @('-b:a','320k','-joint_stereo','0')),
  @('l3-vbr-v0-44k',      44100, 2, 'rich', 0.25, 'VBR V0',          @('-q:a','0')),
  @('l3-vbr-v3-44k',      44100, 2, 'rich', 0.25, 'VBR V3',          @('-q:a','3')),
  @('l3-vbr-v6-44k',      44100, 2, 'rich', 0.25, 'VBR V6',          @('-q:a','6')),
  @('l3-vbr-v9-44k',      44100, 2, 'rich', 0.25, 'VBR V9',          @('-q:a','9')),
  @('l3-abr128-44k-st',   44100, 2, 'rich', 0.25, 'ABR 128',         @('-abr','1','-b:a','128k')),
  @('l3-abr192-48k-st',   48000, 2, 'rich', 0.25, 'ABR 192',         @('-abr','1','-b:a','192k')),
  @('l3-mono-44k-64k',    44100, 1, 'mono', 0.25, 'mono CBR 64',     @('-b:a','64k')),
  @('l3-mono-48k-32k',    48000, 1, 'mono', 0.25, 'mono CBR 32',     @('-b:a','32k')),
  @('l3-joint-44k-128k',  44100, 2, 'corr', 0.25, 'joint CBR 128',   @('-b:a','128k')),
  @('l3-lr-48k-160k',     48000, 2, 'rich', 0.25, 'LR CBR 160',      @('-b:a','160k','-joint_stereo','0')),
  @('l3-lsf-22k-st-64k',  22050, 2, 'rich', 0.40, 'MPEG-2 CBR 64',   @('-b:a','64k')),
  @('l3-lsf-24k-joint-48k',24000,2, 'corr', 0.40, 'MPEG-2 joint 48', @('-b:a','48k')),
  @('l3-lsf-16k-st-32k',  16000, 2, 'rich', 0.40, 'MPEG-2 CBR 32',   @('-b:a','32k')),
  @('l3-m25-11k-st-32k',  11025, 2, 'rich', 0.60, 'MPEG-2.5 CBR 32', @('-b:a','32k')),
  @('l3-m25-12k-mono-24k',12000, 1, 'mono', 0.60, 'MPEG-2.5 mono 24',@('-b:a','24k')),
  @('l3-m25-8k-mono-16k',  8000, 1, 'mono', 1.00, 'MPEG-2.5 mono 16',@('-b:a','16k'))
)
foreach ($c in $l3) {
  $name, $rate, $ch, $kind, $secs, $note, $enc = $c
  $args3 = @('-c:a','libmp3lame') + $enc + @('-write_xing','0')
  $row = Write-MpegCase $name 'mp3' $rate $ch $kind $secs $args3 'mp3float'
  $mrows += "$row|3|$note|$($args3 -join ' ')|$gen"
}
[IO.File]::WriteAllText((Join-Path $mout 'MANIFEST.txt'), (($mrows -join "`n") + "`n"))
Write-Host "wrote $($l2.Count) Layer II and $($l3.Count) Layer III fixtures to $mout"
