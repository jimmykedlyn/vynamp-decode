<#
Dev-only: regenerates tests/fixtures/modern/alac/ (TASK-E01-11) with FFmpeg. FFmpeg is never a
dependency of the crates and is never run by the tests. Writes only that directory.

  $env:FFMPEG = 'D:\Projects\vynamp\.cache\tools\ffmpeg\bin\ffmpeg.exe'   # see engineering/08-reinicio-windows.md (umbrella)
  pwsh tools/fixtures/generate-alac.ps1 [-FFmpeg <path-to-ffmpeg.exe>]

Input: tools/fixtures/source-sine.wav (48 kHz stereo s32, 4800 frames), looped twice (9600
frames, 0.2 s). The source PCM is written first as s16 and s24 stereo WAV and as s16 mono WAV
(left channel); each ALAC file is encoded from one of those WAVs, so the WAV is the exact PCM
the encoder received. The M4B gets its one
chapter from chapters.ffmeta (written here, kept as a fixture: it is the oracle of the chapter
test). Every command runs inside the output directory, and MANIFEST.txt records it verbatim
with the sha256 and the frame count that ffprobe reports. All outputs use +bitexact, so running
the script twice leaves the same bytes.
#>
param([string]$FFmpeg = $env:FFMPEG)
$ErrorActionPreference = 'Stop'
if (-not $FFmpeg) { throw 'ffmpeg path missing: pass -FFmpeg or set $env:FFMPEG' }
$FFmpeg = (Resolve-Path $FFmpeg).Path
$FFprobe = Join-Path (Split-Path -Parent $FFmpeg) ('ffprobe' + [IO.Path]::GetExtension($FFmpeg))
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $root 'tests/fixtures/modern/alac'
New-Item -ItemType Directory -Force $out | Out-Null
$gen = ((& $FFmpeg -version)[0]).Trim()
if ($LASTEXITCODE -ne 0) { throw 'ffmpeg -version failed' }

# One chapter from 100 ms to 200 ms. The start is not 0 on purpose: a reader that drops the
# timestamp would still report 0.
$ffmeta = ";FFMETADATA1`n[CHAPTER]`nTIMEBASE=1/1000`nSTART=100`nEND=200`ntitle=First Chapter`n"
[IO.File]::WriteAllText((Join-Path $out 'chapters.ffmeta'), $ffmeta)

$src = '../../../../tools/fixtures/source-sine.wav'
$bx = @('-fflags', '+bitexact', '-flags:a', '+bitexact')
# name, bits, origin, ffmpeg args (before the output file)
# (The comma binds tighter than `+`, hence the parentheses around each argument list.)
$cases = @(
  ,@('src-s16.wav', 16, 'source-sine.wav', (@('-stream_loop', '1', '-i', $src) + $bx + @('-c:a', 'pcm_s16le')))
  ,@('src-s24.wav', 24, 'source-sine.wav', (@('-stream_loop', '1', '-i', $src) + $bx + @('-c:a', 'pcm_s24le')))
  ,@('src-s16-mono.wav', 16, 'source-sine.wav', (@('-stream_loop', '1', '-i', $src, '-af', 'pan=mono|c0=c0') + $bx + @('-c:a', 'pcm_s16le')))
  ,@('alac-s16.m4a', 16, 'src-s16.wav', (@('-i', 'src-s16.wav') + $bx + @('-c:a', 'alac')))
  ,@('alac-s24.m4a', 24, 'src-s24.wav', (@('-i', 'src-s24.wav') + $bx + @('-c:a', 'alac')))
  ,@('alac-s16-mono.m4a', 16, 'src-s16-mono.wav', (@('-i', 'src-s16-mono.wav') + $bx + @('-c:a', 'alac')))
  ,@('alac-s16-chapters.m4b', 16, 'src-s16.wav+chapters.ffmeta',
    (@('-i', 'src-s16.wav', '-i', 'chapters.ffmeta', '-map', '0:a', '-map_metadata', '1', '-map_chapters', '1') + $bx + @('-c:a', 'alac', '-f', 'ipod')))
)

$rows = @('# arquivo|sha256|rate|channels|bits|frames|origem|comando|gerador')
$sha = (Get-FileHash -Algorithm SHA256 (Join-Path $out 'chapters.ffmeta')).Hash.ToLower()
$rows += "chapters.ffmeta|$sha|-|-|-|-|-|escrito por generate-alac.ps1|-"
Push-Location $out
try {
  foreach ($c in $cases) {
    $name, $bits, $origin, $cmd = $c
    Remove-Item -Force -ErrorAction SilentlyContinue $name
    & $FFmpeg -v error -y @cmd $name
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $name" }
    $probe = & $FFprobe -v error -select_streams a:0 -show_entries stream=sample_rate,channels,duration_ts -of default=nw=1 $name
    if ($LASTEXITCODE -ne 0) { throw "ffprobe failed for $name" }
    $p = @{}
    foreach ($l in $probe) { $k, $v = $l -split '=', 2; $p[$k] = $v }
    $h = (Get-FileHash -Algorithm SHA256 $name).Hash.ToLower()
    $line = 'ffmpeg -v error -y ' + ($cmd -join ' ') + " $name"
    $rows += "$name|$h|$($p['sample_rate'])|$($p['channels'])|$bits|$($p['duration_ts'])|$origin|$line|$gen"
  }
} finally {
  Pop-Location
}
[IO.File]::WriteAllText((Join-Path $out 'MANIFEST.txt'), (($rows -join "`n") + "`n"))
Write-Host "wrote $($cases.Count + 1) fixtures to $out"
