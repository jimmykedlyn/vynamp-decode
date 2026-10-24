<#
Dev-only: regenerates tests/fixtures/pcm/* and tests/fixtures/flac/* (with MANIFEST.txt) with FFmpeg.
FFmpeg is never a dependency of the crates and is never run by the tests.

  . D:\Projects\vynamp-media-player\AMBIENTE.ps1   # sets $env:FFMPEG
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
