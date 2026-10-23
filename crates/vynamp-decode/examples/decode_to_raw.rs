//! Decodes an audio file to interleaved little-endian `f32` samples.
//!
//! Usage: `cargo run --example decode_to_raw -- <input> <output.f32>`

use std::io::Write;
use std::process::ExitCode;
use vynamp_decode::{AudioBuffer, Options, open_path};

fn run(input: &str, output: &str) -> Result<(), String> {
    let mut reader = open_path(std::path::Path::new(input), Options::default())
        .map_err(|e| format!("{input}: {e}"))?;
    let file = std::fs::File::create(output).map_err(|e| format!("{output}: {e}"))?;
    let mut w = std::io::BufWriter::new(file);
    let mut buf = AudioBuffer::new();
    let mut samples = Vec::new();
    let mut bytes = Vec::new();
    while reader.read(&mut buf).map_err(|e| format!("{input}: {e}"))? {
        buf.to_f32_interleaved(&mut samples);
        bytes.clear();
        bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        w.write_all(&bytes).map_err(|e| format!("{output}: {e}"))?;
    }
    w.flush().map_err(|e| format!("{output}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: decode_to_raw <input> <output.f32>");
        return ExitCode::from(1);
    };
    match run(input, output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::from(1)
        }
    }
}
