#[macro_use]
mod diag;

mod cli;
mod commands;
mod gltf_util;
mod matmap;
mod mdx2;
mod objref;
mod scene;
mod selector;
mod shell;
mod vfmt;
mod xform;

use anyhow::{Context, Result};
use clap::Parser;

fn main() {
    if let Err(e) = run() {
        diag::report_error(&e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = cli::Cli::parse();

    let loaded = gltf_util::load(&cli.model, cli.lenient)
        .with_context(|| format!("failed to open model {}", cli.model.display()))?;

    match (&cli.command, &cli.script) {
        (Some(_), Some(_)) => {
            bail_code!(diag::codes::CONFLICTING_MODES, "-c and -s cannot be used together");
        }
        (Some(command), None) => shell::run_all(&loaded, command),
        (None, Some(script_path)) => {
            let contents = std::fs::read_to_string(script_path).map_err(|e| {
                err_code!(
                    diag::codes::SCRIPT_READ_FAILED,
                    "failed to read script {}: {e}",
                    script_path.display()
                )
            })?;
            shell::run_all(&loaded, &contents)
        }
        (None, None) => shell::run_interactive(&loaded),
    }
}
