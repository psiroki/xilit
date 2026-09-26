//! MDX1 binary format writer (spec sections 16-21).

use crate::vfmt::{Binding, Layout, SourceKind};
use anyhow::Result;
use std::collections::HashMap;
use std::io::Write;

// OpenGL enum values used by the MDX1 header/attribute definitions.
const GL_TRIANGLES: u32 = 0x0004;
const GL_UNSIGNED_SHORT: u32 = 0x1403;
const GL_FLOAT: u32 = 0x1406;
const GL_FALSE: u32 = 0;

/// Writes an MDX1 file to `out` for a single triangle-list primitive.
///
/// * `layout` — the resolved vertex layout (spec sections 9-15).
/// * `vertex_count` — number of vertices (must match every source array's length).
/// * `sources` — per-`SourceKind` per-vertex float rows, as read from the glTF accessors.
/// * `indices` — the (already validated to fit in u16) vertex indices, in glTF winding order.
pub fn write_mdx1<W: Write>(
    out: &mut W,
    layout: &Layout,
    vertex_count: usize,
    sources: &HashMap<SourceKind, Vec<Vec<f32>>>,
    indices: &[u16],
) -> Result<()> {
    // Build the string pool (spec section 17): unique emitted attribute
    // names, in first-appearance order, NUL-terminated.
    let mut string_pool = Vec::new();
    let mut name_offsets: HashMap<String, u32> = HashMap::new();
    for entry in &layout.entries {
        if !entry.emits_attribute {
            continue;
        }
        if !name_offsets.contains_key(&entry.field.name) {
            let offset = string_pool.len() as u32;
            name_offsets.insert(entry.field.name.clone(), offset);
            string_pool.extend_from_slice(entry.field.name.as_bytes());
            string_pool.push(0);
        }
    }
    let padding = (4 - (string_pool.len() & 3)) & 3;
    string_pool.extend(std::iter::repeat(0).take(padding));

    let attribute_defs: Vec<&crate::vfmt::LayoutEntry> =
        layout.entries.iter().filter(|e| e.emits_attribute).collect();

    // Header (spec section 16): 8 x u32 = 32 bytes.
    out.write_all(b"MDX1")?;
    write_u32(out, vertex_count as u32)?;
    write_u32(out, indices.len() as u32)?;
    write_u32(out, attribute_defs.len() as u32)?;
    write_u32(out, layout.vertex_size)?;
    write_u32(out, GL_TRIANGLES)?;
    write_u32(out, GL_UNSIGNED_SHORT)?;
    write_u32(out, string_pool.len() as u32)?;

    // String pool.
    out.write_all(&string_pool)?;

    // Vertex attribute definitions (spec section 18): 5 x u32 each.
    for entry in &attribute_defs {
        let name_offset = name_offsets[&entry.field.name];
        write_u32(out, name_offset)?;
        write_u32(out, entry.component_count)?;
        write_u32(out, GL_FLOAT)?;
        write_u32(out, GL_FALSE)?;
        write_u32(out, entry.offset)?;
    }

    // Vertex buffer (spec section 19).
    for v in 0..vertex_count {
        for entry in &layout.entries {
            match entry.field.binding {
                Binding::Source(kind) => {
                    let row = &sources[&kind][v];
                    for &f in row {
                        out.write_all(&f.to_le_bytes())?;
                    }
                }
                Binding::App(_) => {
                    out.write_all(&vec![0u8; entry.size as usize])?;
                }
                Binding::Pad(_) => {
                    out.write_all(&vec![0u8; entry.size as usize])?;
                }
            }
        }
    }

    // Index buffer (spec section 20).
    for &idx in indices {
        out.write_all(&idx.to_le_bytes())?;
    }

    Ok(())
}

fn write_u32<W: Write>(out: &mut W, v: u32) -> Result<()> {
    out.write_all(&v.to_le_bytes())?;
    Ok(())
}
