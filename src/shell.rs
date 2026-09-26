//! Command dispatch shared by the interactive shell, `-c`, and `-s` modes
//! (spec section 2).

use crate::gltf_util::LoadedGltf;
use crate::objref;
use anyhow::{bail, Result};
use std::io::Write;

/// Run every command found in `input` (splitting on top-level `;`/newline).
/// Stops and returns the first error encountered (non-interactive
/// semantics: "an error immediately terminates execution").
pub fn run_all(loaded: &LoadedGltf, input: &str) -> Result<()> {
    for raw_cmd in objref::split_commands(input) {
        dispatch(loaded, &raw_cmd)?;
    }
    Ok(())
}

/// Run the interactive shell: read commands from stdin until EOF, printing
/// errors but continuing (spec section 2: "Errors are displayed and the
/// shell remains active so the user can correct and repeat commands").
pub fn run_interactive(loaded: &LoadedGltf) -> Result<()> {
    let stdin = std::io::stdin();
    loop {
        print!("xilit> ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
        let bytes_read = stdin.read_line(&mut line)?;
        if bytes_read == 0 {
            println!();
            break;
        }
        let line = line.trim_end_matches(['\n', '\r']);
        if line.trim().is_empty() {
            continue;
        }

        for raw_cmd in objref::split_commands(line) {
            if let Err(e) = dispatch(loaded, &raw_cmd) {
                eprintln!("error: {e:#}");
            }
        }
    }
    Ok(())
}

fn dispatch(loaded: &LoadedGltf, raw_cmd: &str) -> Result<()> {
    let tokens = objref::tokenize_raw(raw_cmd)?;
    let Some(first) = tokens.first() else {
        return Ok(());
    };

    match first.as_str() {
        "list" => {
            require_no_more_args(&tokens, 1)?;
            crate::commands::list::overview(loaded);
        }
        "mesh" => match tokens.get(1).map(String::as_str) {
            Some("list") => {
                require_no_more_args(&tokens, 2)?;
                crate::commands::list::meshes(loaded);
            }
            Some("export") => crate::commands::mesh::export(loaded, &tokens[2..])?,
            _ => bail!(
                "usage: mesh list | mesh export <mesh>/<primitiveIndex> <filename.mdx> [--vfmt <vertexFormat>]"
            ),
        },
        "image" => match tokens.get(1).map(String::as_str) {
            Some("list") => {
                require_no_more_args(&tokens, 2)?;
                crate::commands::list::images(loaded);
            }
            _ => bail!("usage: image list"),
        },
        "material" => match tokens.get(1).map(String::as_str) {
            Some("list") => {
                require_no_more_args(&tokens, 2)?;
                crate::commands::list::materials(loaded);
            }
            _ => bail!("usage: material list"),
        },
        "texture" => match tokens.get(1).map(String::as_str) {
            Some("list") => {
                require_no_more_args(&tokens, 2)?;
                crate::commands::list::textures(loaded);
            }
            _ => bail!("usage: texture list"),
        },
        "node" => match tokens.get(1).map(String::as_str) {
            Some("list") => {
                require_no_more_args(&tokens, 2)?;
                crate::commands::list::nodes(loaded);
            }
            _ => bail!("usage: node list"),
        },
        "scene" => match tokens.get(1).map(String::as_str) {
            Some("list") => {
                require_no_more_args(&tokens, 2)?;
                crate::commands::list::scenes(loaded);
            }
            _ => bail!("usage: scene list"),
        },
        other => bail!("unknown command: {other}"),
    }

    Ok(())
}

fn require_no_more_args(tokens: &[String], expected_len: usize) -> Result<()> {
    if tokens.len() != expected_len {
        bail!(
            "unexpected extra argument(s): {}",
            tokens[expected_len..].join(" ")
        );
    }
    Ok(())
}
