//! Aurum CLI entry point.

use aurum::cli::{self, Cli};
use clap::Parser;
use std::process::ExitCode;

/// Windows default stack is 1 MiB; `converse --stdio` futures overflow it
/// (`STATUS_STACK_OVERFLOW` / `-1073741571`).
const WORKER_STACK: usize = 4 * 1024 * 1024;

fn main() -> ExitCode {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(WORKER_STACK)
        .build()
        .expect("tokio runtime");
    rt.block_on(async_main())
}

async fn async_main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli::run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            cli::report_error(&err);
            ExitCode::from(err.exit_code() as u8)
        }
    };
    // Prevent Metal/ggml teardown asserts from process-global model cache Drop.
    aurum_core::providers::local::clear_context_cache();
    code
}
