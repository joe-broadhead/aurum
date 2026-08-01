//! `aurum support-bundle` — privacy-safe diagnostics (JOE-1728).

use aurum_core::error::Result;
use aurum_core::support::default_bundle_path;
use clap::Parser;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

/// Write a redacted offline support bundle (no audio, keys, or transcripts).
///
/// Machine fields use allowlist redaction. Optional `--notes` are untrusted
/// free-form input (token/path scrubbed, not privacy-guaranteed).
#[derive(Debug, Parser)]
pub struct SupportBundleCli {
    /// Output path (default: aurum-support-<unix>.json in cwd).
    #[arg(long = "output-file", short = 'O', value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Optional untrusted notes (do not paste secrets, transcripts, or private paths).
    /// Notes are scrubbed for common token/path shapes but are not privacy-guaranteed.
    #[arg(long = "notes", value_name = "TEXT")]
    pub notes: Option<String>,

    /// Print JSON to stdout instead of writing a file.
    #[arg(long)]
    pub stdout: bool,
}

pub fn run_support_bundle(cli: SupportBundleCli) -> Result<()> {
    let engine = aurum_core::AurumEngine::load()?;
    let bundle = engine.support_bundle(cli.notes);
    if cli.stdout {
        println!("{}", bundle.to_json_pretty()?);
        return Ok(());
    }
    let path = cli.output_file.unwrap_or_else(default_bundle_path);
    bundle.write_to_path(&path)?;
    if io::stderr().is_terminal() {
        eprintln!("aurum: wrote redacted support bundle {}", path.display());
        eprintln!(
            "aurum: machine fields exclude audio/keys; --notes are untrusted free-form input"
        );
    }
    Ok(())
}
