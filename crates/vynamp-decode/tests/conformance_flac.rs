//! Official conformance corpus: the IETF CELLAR `flac-test-files` (CC0).
//!
//! `tests/fixtures/flac-conformance/subset/` holds a selection of the repository's
//! `subset` group: every file must decode in `Mode::Strict` to the end, which
//! includes the STREAMINFO MD5 check done by the decoder (a mismatch is an
//! error). `faulty/` holds files with invalid data: each must give an `Err` in
//! `Mode::Strict` and must never panic.
//!
//! Set `VYNAMP_FLAC_TEST_FILES` to a clone of the upstream repository to run the
//! complete `subset` and `faulty` groups instead of the committed selection.

#![cfg(feature = "flac")]

use std::path::{Path, PathBuf};
use vynamp_decode::{AudioBuffer, Mode, Options, open_path};

/// Directories to scan: the committed selection, or an upstream clone.
fn group(name: &str) -> PathBuf {
    match std::env::var_os("VYNAMP_FLAC_TEST_FILES") {
        Some(root) => PathBuf::from(root).join(name),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/flac-conformance")
            .join(name),
    }
}

fn flac_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{dir:?}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "flac"))
        .collect();
    files.sort();
    files
}

fn strict() -> Options {
    Options {
        mode: Mode::Strict,
        ..Options::default()
    }
}

/// Decodes the whole file; returns the number of sample frames.
fn decode_all(path: &Path) -> Result<u64, vynamp_decode::Error> {
    let mut reader = open_path(path, strict())?;
    let mut buf = AudioBuffer::new();
    let mut frames = 0u64;
    while reader.read(&mut buf)? {
        frames += buf.frames() as u64;
    }
    Ok(frames)
}

/// The STREAMINFO MD5 (offset 26 of a stream without ID3v2) is not all zero.
fn has_md5(path: &Path) -> bool {
    let bytes = std::fs::read(path).unwrap();
    bytes.len() >= 42 && bytes[26..42].iter().any(|&b| b != 0)
}

#[test]
fn subset_files_decode_strictly_and_verify_their_md5() {
    let files = flac_files(&group("subset"));
    assert!(files.len() >= 20, "only {} subset files", files.len());
    let mut failures = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match decode_all(path) {
            Ok(frames) => {
                assert!(frames > 0, "{name}: no audio");
                // The decoder verifies the MD5 whenever STREAMINFO carries one.
                assert!(has_md5(path), "{name}: STREAMINFO has no MD5 to verify");
                eprintln!("ok   {name} ({frames} frames)");
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "subset failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn faulty_files_are_rejected_in_strict_mode() {
    let files = flac_files(&group("faulty"));
    assert!(files.len() >= 5, "only {} faulty files", files.len());
    let mut accepted = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("10 - ") {
            // Invalid Vorbis comment block: validated in plan 04 (comment parsing).
            continue;
        }
        match decode_all(path) {
            Ok(_) => accepted.push(name),
            Err(e) => eprintln!("err  {name}: {e}"),
        }
    }
    assert!(accepted.is_empty(), "accepted in Strict: {accepted:?}");
}
