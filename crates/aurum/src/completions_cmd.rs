//! Shell completions and man page generation (JOE-1720).

use crate::cli::Cli;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, shells};
use std::io::{self, Write};

#[derive(Debug, Parser)]
pub struct CompletionsCli {
    /// Shell: bash | zsh | fish | powershell | elvish
    #[arg(value_name = "SHELL")]
    pub shell: String,
}

pub fn run_completions(cli: CompletionsCli) -> Result<(), String> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    let shell = cli.shell.to_ascii_lowercase();
    let mut stdout = io::stdout().lock();
    match shell.as_str() {
        "bash" => generate(shells::Bash, &mut cmd, &name, &mut stdout),
        "zsh" => generate(shells::Zsh, &mut cmd, &name, &mut stdout),
        "fish" => generate(shells::Fish, &mut cmd, &name, &mut stdout),
        "powershell" | "pwsh" => generate(shells::PowerShell, &mut cmd, &name, &mut stdout),
        "elvish" => generate(shells::Elvish, &mut cmd, &name, &mut stdout),
        other => {
            return Err(format!(
                "unknown shell '{other}' (use bash|zsh|fish|powershell|elvish)"
            ));
        }
    }
    stdout.flush().ok();
    Ok(())
}

/// Generate a man page on stdout.
pub fn run_man() -> Result<(), String> {
    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buffer: Vec<u8> = Vec::new();
    man.render(&mut buffer)
        .map_err(|e| format!("render man page: {e}"))?;
    io::stdout()
        .write_all(&buffer)
        .map_err(|e| format!("write man page: {e}"))?;
    Ok(())
}
