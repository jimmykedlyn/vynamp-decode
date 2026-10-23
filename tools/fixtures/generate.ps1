<#
Dev-only: regenerates tests/fixtures/pcm/* and MANIFEST.txt with FFmpeg.
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
