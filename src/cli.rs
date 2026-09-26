use clap::Parser;
use std::path::PathBuf;

/// xilit — inspect glTF 2.0 / GLB models and export mesh primitives to MDX1.
#[derive(Parser, Debug)]
#[command(name = "xilit", version, about)]
pub struct Cli {
    /// The .glb (or .gltf) model to open.
    pub model: PathBuf,

    /// Execute the given command(s) and exit. Multiple commands may be
    /// separated by ';' or newlines.
    #[arg(short = 'c', long = "command", value_name = "COMMANDS")]
    pub command: Option<String>,

    /// Execute commands from a script file and exit.
    #[arg(short = 's', long = "script", value_name = "FILE")]
    pub script: Option<PathBuf>,
}
