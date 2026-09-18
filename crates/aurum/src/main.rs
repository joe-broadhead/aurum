//! Aurum CLI entry point.

use aurum::cli::{self, Cli};
use clap::Parser;
use std::process::ExitCode;

/// Windows default stack is 1 MiB; `converse --stdio` overflows it on the
/// process main thread (`STATUS_STACK_OVERFLOW` / `thread 'main' has overflowed`).
/// Tokio `thread_stack_size` does not apply to `block_on`'s caller.
const WORKER_STACK: usize = 8 * 1024 * 1024;

fn main() -> ExitCode {
    std::thread::Builder::new()
        .name("aurum-main".into())
        .stack_size(WORKER_STACK)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(WORKER_STACK)
                .build()
                .expect("tokio runtime");
            rt.block_on(async_main())
        })
        .expect("spawn aurum-main")
        .join()
        .unwrap_or(ExitCode::from(1))
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
