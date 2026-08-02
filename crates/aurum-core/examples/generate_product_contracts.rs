//! Generate or check product contract surfaces (JOE-2224).
//!
//! ```bash
//! cargo run -p aurum-core --example generate_product_contracts -- --check
//! cargo run -p aurum-core --example generate_product_contracts -- --write
//! ```
//!
//! Offline: uses only compile-time registries and VERSION/Cargo metadata.

use aurum_core::product_contracts::ProductContractsSnapshot;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn repo_root() -> PathBuf {
    // examples run from package dir or workspace; walk up for VERSION.
    let mut dir = env::current_dir().expect("cwd");
    for _ in 0..6 {
        if dir.join("VERSION").is_file() && dir.join("docs").is_dir() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    // Fallback: CARGO_MANIFEST_DIR/../..
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| env::current_dir().unwrap())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let write = args.iter().any(|a| a == "--write");
    let check = args.iter().any(|a| a == "--check") || !write;

    let snap = match ProductContractsSnapshot::builtin() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = snap.validate() {
        eprintln!("validation failed: {e}");
        return ExitCode::FAILURE;
    }

    let root = repo_root();
    let json_path = root.join("docs/generated/product-contracts.json");
    let md_path = root.join("docs/generated/provider-matrix.generated.md");

    let json = match snap.to_json_pretty() {
        Ok(j) => {
            let mut j = j;
            if !j.ends_with('\n') {
                j.push('\n');
            }
            j
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let md = {
        let mut m = snap.to_markdown_matrix();
        if !m.ends_with('\n') {
            m.push('\n');
        }
        m
    };

    if write {
        if let Some(p) = json_path.parent() {
            let _ = fs::create_dir_all(p);
        }
        if let Err(e) = fs::write(&json_path, &json) {
            eprintln!("write {}: {e}", json_path.display());
            return ExitCode::FAILURE;
        }
        if let Err(e) = fs::write(&md_path, &md) {
            eprintln!("write {}: {e}", md_path.display());
            return ExitCode::FAILURE;
        }
        println!("wrote {}", json_path.display());
        println!("wrote {}", md_path.display());
        return ExitCode::SUCCESS;
    }

    if check {
        let mut ok = true;
        match fs::read_to_string(&json_path) {
            Ok(existing) => {
                if existing != json {
                    eprintln!("DRIFT: {}", json_path.display());
                    ok = false;
                }
            }
            Err(_) => {
                eprintln!("MISSING: {}", json_path.display());
                ok = false;
            }
        }
        match fs::read_to_string(&md_path) {
            Ok(existing) => {
                if existing != md {
                    eprintln!("DRIFT: {}", md_path.display());
                    ok = false;
                }
            }
            Err(_) => {
                eprintln!("MISSING: {}", md_path.display());
                ok = false;
            }
        }
        if !ok {
            eprintln!(
                "hint: cargo run -p aurum-core --example generate_product_contracts -- --write"
            );
            return ExitCode::FAILURE;
        }
        println!(
            "OK product contracts match registry (schema {})",
            snap.schema_version
        );
        return ExitCode::SUCCESS;
    }

    ExitCode::SUCCESS
}
