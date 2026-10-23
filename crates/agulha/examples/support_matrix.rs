//! Prints the support matrix of the default registry as Markdown.
//!
//! Usage: `cargo run --example support_matrix`

use agulha::{Verification, default_registry};

fn level(v: Verification) -> &'static str {
    match v {
        Verification::Conformance => "conformance",
        Verification::Oracle => "oracle",
        Verification::Experimental => "experimental",
    }
}

fn main() {
    let reg = default_registry();
    println!("| Kind | Name | Extensions | Verification |");
    println!("|------|------|------------|--------------|");
    for c in reg.containers() {
        println!(
            "| container | {} | {} | - |",
            c.name,
            c.extensions
                .iter()
                .map(|e| format!(".{e}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    for c in reg.codecs() {
        println!("| codec | {} | - | {} |", c.name, level(c.verification));
    }
}
