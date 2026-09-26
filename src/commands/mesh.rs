//! `mesh export <mesh>/<primitiveIndex> <filename.mdx> [--vfmt <vertexFormat>]`
//! (spec sections 5-8, 22).

use crate::gltf_util::{self, LoadedGltf};
use crate::objref::ObjRef;
use crate::vfmt::{self, Layout, SourceKind};
use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;

pub const DEFAULT_VFMT: &str = "position,normal,uv0";

/// Map a glTF attribute [`gltf::Semantic`] to its on-the-wire attribute
/// name (`POSITION`, `TEXCOORD_0`, ...).
pub fn semantic_key(sem: &gltf::Semantic) -> String {
    use gltf::Semantic;
    match sem {
        Semantic::Positions => "POSITION".to_string(),
        Semantic::Normals => "NORMAL".to_string(),
        Semantic::Tangents => "TANGENT".to_string(),
        Semantic::TexCoords(n) => format!("TEXCOORD_{n}"),
        Semantic::Colors(n) => format!("COLOR_{n}"),
        Semantic::Joints(n) => format!("JOINTS_{n}"),
        Semantic::Weights(n) => format!("WEIGHTS_{n}"),
        Semantic::Extras(name) => name.clone(),
    }
}

fn resolve_mesh<'a>(loaded: &'a LoadedGltf, objref: &ObjRef) -> Result<gltf::Mesh<'a>> {
    match objref {
        ObjRef::Index(i) => loaded
            .document
            .meshes()
            .nth(*i)
            .ok_or_else(|| anyhow!("no mesh at index #{i}")),
        ObjRef::Name(name) => loaded
            .document
            .meshes()
            .find(|m| m.name() == Some(name.as_str()))
            .ok_or_else(|| anyhow!("no mesh named {name:?}")),
    }
}

/// Parse and run `mesh export ...` given the tokens *after* the literal
/// `export` keyword (still raw/quoted, as produced by
/// [`crate::objref::tokenize_raw`]).
pub fn export(loaded: &LoadedGltf, args: &[String]) -> Result<()> {
    if args.len() < 2 {
        bail!("usage: mesh export <mesh>/<primitiveIndex> <filename.mdx> [--vfmt <vertexFormat>]");
    }

    let (mesh_ref, prim_index) = crate::objref::parse_objref_with_index(&args[0])
        .with_context(|| format!("invalid mesh/primitive reference: {}", args[0]))?;
    let out_path = crate::objref::unquote_token(&args[1])?;

    let mut vfmt_str = DEFAULT_VFMT.to_string();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--vfmt" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow!("--vfmt requires a value"))?;
                vfmt_str = crate::objref::unquote_token(value)?;
                i += 2;
            }
            other => bail!("unexpected argument: {other}"),
        }
    }

    run_export(loaded, &mesh_ref, prim_index, Path::new(&out_path), &vfmt_str)
}

fn run_export(
    loaded: &LoadedGltf,
    mesh_ref: &ObjRef,
    prim_index: usize,
    out_path: &Path,
    vfmt_str: &str,
) -> Result<()> {
    let mesh = resolve_mesh(loaded, mesh_ref)?;
    let primitives: Vec<_> = mesh.primitives().collect();
    let prim = primitives.get(prim_index).ok_or_else(|| {
        anyhow!(
            "mesh {mesh_ref} has {} primitive(s); no primitive #{prim_index}",
            primitives.len()
        )
    })?;

    if prim.mode() != gltf::mesh::Mode::Triangles {
        bail!(
            "primitive {mesh_ref}/{prim_index} uses mode {:?}; MDX1 only supports TRIANGLES",
            prim.mode()
        );
    }

    let indices_accessor = prim.indices().ok_or_else(|| {
        anyhow!("primitive {mesh_ref}/{prim_index} has no index accessor (non-indexed primitives are not supported)")
    })?;
    let raw_indices = gltf_util::read_index_accessor(&loaded.buffers, &indices_accessor)?;
    let mut indices_u16 = Vec::with_capacity(raw_indices.len());
    for v in raw_indices {
        if v > u16::MAX as u32 {
            bail!("index {v} does not fit into MDX1's u16 index type");
        }
        indices_u16.push(v as u16);
    }

    // Parse & validate the vertex format (spec sections 9-15).
    let fields = vfmt::parse_vfmt(vfmt_str)?;
    for field in &fields {
        if !field.is_hidden() && !vfmt::looks_like_glsl_identifier(&field.name) {
            eprintln!(
                "warning: emitted attribute name {:?} does not look like a valid GLSL variable identifier",
                field.name
            );
        }
    }
    let layout = Layout::build(fields)?;

    // Map glTF attribute semantics -> accessor, for this primitive.
    let attributes: HashMap<String, gltf::Accessor> = prim
        .attributes()
        .map(|(sem, acc)| (semantic_key(&sem), acc))
        .collect();

    let position_accessor = attributes
        .get("POSITION")
        .ok_or_else(|| anyhow!("primitive {mesh_ref}/{prim_index} has no POSITION attribute"))?;
    let vertex_count = position_accessor.count();

    // Gather every distinct source binding the vertex format needs, read
    // and validate each against the primitive's accessors (spec section 8).
    let mut sources: HashMap<SourceKind, Vec<Vec<f32>>> = HashMap::new();
    for entry in &layout.entries {
        let Some(kind) = entry.source_kind() else {
            continue;
        };
        if sources.contains_key(&kind) {
            continue;
        }
        let attr_name = kind.gltf_attribute_name();
        let accessor = attributes.get(&attr_name).ok_or_else(|| {
            anyhow!(
                "vertex format requests '{kind}', but primitive {mesh_ref}/{prim_index} has no {attr_name} attribute"
            )
        })?;

        let rows = gltf_util::read_float_accessor(&loaded.buffers, accessor).with_context(|| {
            format!("reading source attribute '{kind}' ({attr_name})")
        })?;

        if rows.len() != vertex_count {
            bail!(
                "source attribute '{kind}' ({attr_name}) has {} vertices, but POSITION has {vertex_count}",
                rows.len()
            );
        }
        let expected_components = kind.component_count() as usize;
        if let Some(first) = rows.first() {
            if first.len() != expected_components {
                bail!(
                    "source attribute '{kind}' ({attr_name}) has {} components per vertex, expected {expected_components}",
                    first.len()
                );
            }
        }

        sources.insert(kind, rows);
    }

    let file = std::fs::File::create(out_path)
        .with_context(|| format!("failed to create output file {}", out_path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    crate::mdx::write_mdx1(&mut writer, &layout, vertex_count, &sources, &indices_u16)?;
    use std::io::Write;
    writer.flush()?;

    println!(
        "exported {mesh_ref}/{prim_index} -> {} ({vertex_count} vertices, {} indices, {} bytes/vertex)",
        out_path.display(),
        indices_u16.len(),
        layout.vertex_size
    );

    Ok(())
}
