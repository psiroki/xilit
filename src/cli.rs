use clap::Parser;
use std::path::PathBuf;

/// xilit — inspect glTF 2.0 / GLB models and export them to MDX2.
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

    /// Don't reject models that fail glTF validation (e.g. unsupported
    /// required extensions such as EXT_texture_webp). Problems are printed
    /// as warnings; export only fails if it actually touches broken data.
    #[arg(long)]
    pub lenient: bool,
}
