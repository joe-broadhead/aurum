//! Aurum CLI entry point.

use aurum::cli::{self, Cli};
use clap::Parser;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli::run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            cli::report_error(&err);
            ExitCode::from(err.exit_code() as u8)
        }
    }
}
