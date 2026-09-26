mod cli;
mod commands;
mod gltf_util;
mod mdx;
mod objref;
mod shell;
mod vfmt;

use anyhow::{Context, Result};
use clap::Parser;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = cli::Cli::parse();

    let loaded = gltf_util::load(&cli.model)
        .with_context(|| format!("failed to open model {}", cli.model.display()))?;

    match (&cli.command, &cli.script) {
        (Some(_), Some(_)) => {
            anyhow::bail!("-c and -s cannot be used together");
        }
        (Some(command), None) => shell::run_all(&loaded, command),
        (None, Some(script_path)) => {
            let contents = std::fs::read_to_string(script_path)
                .with_context(|| format!("failed to read script {}", script_path.display()))?;
            shell::run_all(&loaded, &contents)
        }
        (None, None) => shell::run_interactive(&loaded),
    }
}
