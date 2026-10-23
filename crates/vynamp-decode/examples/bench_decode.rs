//! Decodes a file repeatedly and reports throughput.
//!
//! Usage: `cargo run --release --example bench_decode -- <file> [iterations]`
//!
//! One warm-up iteration is run first. Each timed iteration opens the file
//! (`open_path`) and decodes it to the end, so file I/O is included. Reports
//! min and median over the iterations as MB/s of input bytes (1 MB = 10^6
//! bytes) and as multiples of realtime (decoded frames / sample rate /
//! elapsed). The numbers are informative, not a gate.

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use vynamp_decode::{AudioBuffer, Options, open_path};

/// Decodes `path` to the end; returns (decoded frames, sample rate).
fn decode_once(path: &Path) -> Result<(u64, u32), String> {
    let mut reader =
        open_path(path, Options::default()).map_err(|e| format!("{}: {e}", path.display()))?;
    let rate = reader.track().sample_rate;
    let mut buf = AudioBuffer::new();
    let mut frames = 0u64;
    while reader
        .read(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?
    {
        frames += buf.frames() as u64;
    }
    Ok((frames, rate))
}

fn run(path: &str, iterations: usize) -> Result<(), String> {
    let path = Path::new(path);
    let bytes = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    decode_once(path)?; // warm-up
    let mut times: Vec<Duration> = Vec::with_capacity(iterations);
    let mut info = (0u64, 0u32);
    for _ in 0..iterations {
        let start = Instant::now();
        info = decode_once(path)?;
        times.push(start.elapsed());
    }
    times.sort();
    let (frames, rate) = info;
    let audio_secs = if rate == 0 {
        0.0
    } else {
        frames as f64 / f64::from(rate)
    };
    let report = |label: &str, t: Duration| {
        let secs = t.as_secs_f64().max(f64::MIN_POSITIVE);
        println!(
            "{label:>6}: {:>9.3} ms  {:>9.1} MB/s  {:>9.1}x realtime",
            secs * 1e3,
            bytes as f64 / 1e6 / secs,
            audio_secs / secs
        );
    };
    println!(
        "{}: {bytes} bytes, {frames} frames @ {rate} Hz, {iterations} iterations",
        path.display()
    );
    report("min", times[0]);
    report("median", times[times.len() / 2]);
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (file, iterations) = match args.as_slice() {
        [f] => (f, Ok(5)),
        [f, n] => (f, n.parse::<usize>()),
        _ => {
            eprintln!("usage: bench_decode <file> [iterations]");
            return ExitCode::from(1);
        }
    };
    let iterations = match iterations {
        Ok(n) if n > 0 => n,
        _ => {
            eprintln!("error: iterations must be a positive integer");
            return ExitCode::from(1);
        }
    };
    match run(file, iterations) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::from(1)
        }
    }
}
