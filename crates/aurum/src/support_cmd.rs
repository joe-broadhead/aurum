//! `aurum support-bundle` — privacy-safe diagnostics (JOE-1728).

use aurum_core::error::Result;
use aurum_core::support::default_bundle_path;
use clap::Parser;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

/// Write a redacted offline support bundle (no audio, keys, or transcripts).
#[derive(Debug, Parser)]
pub struct SupportBundleCli {
    /// Output path (default: aurum-support-<unix>.json in cwd).
    #[arg(long = "output-file", short = 'O', value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Optional public notes to embed (do not paste secrets or transcripts).
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
        eprintln!(
            "aurum: wrote privacy-safe support bundle {}",
            path.display()
        );
        eprintln!("aurum: attach this file to a GitHub issue; it excludes audio and secrets");
    }
    Ok(())
}
