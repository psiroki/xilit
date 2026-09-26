//! Minimal glTF/GLB loading and accessor data reading.
//!
//! We deliberately avoid the `gltf` crate's `import` feature: it pulls in
//! `image` (with PNG/JPEG decoders) purely to decode texture pixels, which
//! xilit never needs (it only lists image/texture *metadata*, spec section
//! 3). Instead we parse the document with the crate's core (always
//! available) `Gltf::from_slice`, then resolve buffers ourselves.

use anyhow::{anyhow, bail, Context, Result};
use gltf::Document;
use std::path::{Path, PathBuf};

pub struct LoadedGltf {
    pub document: Document,
    pub buffers: Vec<Vec<u8>>,
    #[allow(dead_code)]
    pub base_dir: PathBuf,
}

pub fn load(path: &Path) -> Result<LoadedGltf> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let gltf = gltf::Gltf::from_slice(&bytes)
        .with_context(|| format!("failed to parse glTF/GLB: {}", path.display()))?;
    let base_dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let document = gltf.document;
    let glb_blob = gltf.blob;

    let mut buffers = Vec::with_capacity(document.buffers().count());
    for buffer in document.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => glb_blob.clone().ok_or_else(|| {
                anyhow!(
                    "buffer #{} refers to the GLB binary chunk, but none is present",
                    buffer.index()
                )
            })?,
            gltf::buffer::Source::Uri(uri) => load_uri(uri, &base_dir)
                .with_context(|| format!("failed to load buffer #{}", buffer.index()))?,
        };
        if data.len() < buffer.length() {
            bail!(
                "buffer #{} is shorter than declared ({} < {} bytes)",
                buffer.index(),
                data.len(),
                buffer.length()
            );
        }
        buffers.push(data);
    }

    Ok(LoadedGltf {
        document,
        buffers,
        base_dir,
    })
}

fn load_uri(uri: &str, base_dir: &Path) -> Result<Vec<u8>> {
    if let Some(data) = uri.strip_prefix("data:") {
        let comma = data
            .find(',')
            .ok_or_else(|| anyhow!("malformed data: URI (no comma)"))?;
        let (meta, payload) = (&data[..comma], &data[comma + 1..]);
        if !meta.ends_with(";base64") {
            bail!("unsupported data: URI (only base64-encoded payloads are supported)");
        }
        return base64::decode(payload).context("failed to base64-decode data: URI");
    }

    let decoded = percent_decode(uri);
    let full_path = base_dir.join(decoded);
    std::fs::read(&full_path)
        .with_context(|| format!("failed to read external buffer file {}", full_path.display()))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read the raw byte slice backing `accessor`, respecting the buffer view's
/// byte offset/stride, but *not* interpreting it. Returns an error for
/// sparse accessors (unsupported, spec section 4) or accessors without a
/// buffer view.
fn accessor_bytes<'a>(
    buffers: &'a [Vec<u8>],
    accessor: &gltf::Accessor,
) -> Result<(&'a [u8], usize, usize)> {
    if accessor.sparse().is_some() {
        bail!(
            "accessor #{} uses sparse storage, which xilit does not support",
            accessor.index()
        );
    }
    let view = accessor
        .view()
        .ok_or_else(|| anyhow!("accessor #{} has no buffer view", accessor.index()))?;
    let buffer = view.buffer();
    let buf_data = buffers.get(buffer.index()).ok_or_else(|| {
        anyhow!(
            "accessor #{} refers to out-of-range buffer #{}",
            accessor.index(),
            buffer.index()
        )
    })?;

    let component_size = data_type_size(accessor.data_type())?;
    let component_count = dimensions_count(accessor.dimensions());
    let default_stride = component_size * component_count;
    let stride = view.stride().unwrap_or(default_stride);

    let base = view.offset() + accessor.offset();
    let needed = base + stride.max(default_stride) * accessor.count().saturating_sub(1)
        + default_stride;
    if needed > buf_data.len() {
        bail!(
            "accessor #{} extends past the end of its buffer ({} > {} bytes)",
            accessor.index(),
            needed,
            buf_data.len()
        );
    }

    Ok((buf_data, base, stride))
}

fn data_type_size(dt: gltf::accessor::DataType) -> Result<usize> {
    use gltf::accessor::DataType;
    Ok(match dt {
        DataType::I8 | DataType::U8 => 1,
        DataType::I16 | DataType::U16 => 2,
        DataType::U32 | DataType::F32 => 4,
    })
}

fn dimensions_count(d: gltf::accessor::Dimensions) -> usize {
    use gltf::accessor::Dimensions;
    match d {
        Dimensions::Scalar => 1,
        Dimensions::Vec2 => 2,
        Dimensions::Vec3 => 3,
        Dimensions::Vec4 => 4,
        Dimensions::Mat2 => 4,
        Dimensions::Mat3 => 9,
        Dimensions::Mat4 => 16,
    }
}

/// Read a floating-point vertex attribute accessor as `count` rows of
/// `component_count` `f32`s each. Errors if the accessor's component type
/// is not `FLOAT` (spec section 8: "All source vertex attributes used for
/// MDX1 export must contain floating-point data").
pub fn read_float_accessor(
    buffers: &[Vec<u8>],
    accessor: &gltf::Accessor,
) -> Result<Vec<Vec<f32>>> {
    if accessor.data_type() != gltf::accessor::DataType::F32 {
        bail!(
            "accessor #{} is not floating-point (found {:?}); xilit does not convert or \
             normalize non-float source data",
            accessor.index(),
            accessor.data_type()
        );
    }
    let component_count = dimensions_count(accessor.dimensions());
    let (data, base, stride) = accessor_bytes(buffers, accessor)?;

    let mut rows = Vec::with_capacity(accessor.count());
    for i in 0..accessor.count() {
        let row_start = base + i * stride;
        let mut row = Vec::with_capacity(component_count);
        for c in 0..component_count {
            let off = row_start + c * 4;
            let f = f32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            row.push(f);
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Read an index accessor (`UNSIGNED_BYTE`/`UNSIGNED_SHORT`/`UNSIGNED_INT`,
/// spec section 7) as a plain `Vec<u32>`.
pub fn read_index_accessor(buffers: &[Vec<u8>], accessor: &gltf::Accessor) -> Result<Vec<u32>> {
    use gltf::accessor::DataType;
    let (data, base, stride) = accessor_bytes(buffers, accessor)?;
    let mut out = Vec::with_capacity(accessor.count());
    for i in 0..accessor.count() {
        let off = base + i * stride;
        let v = match accessor.data_type() {
            DataType::U8 => data[off] as u32,
            DataType::U16 => u16::from_le_bytes(data[off..off + 2].try_into().unwrap()) as u32,
            DataType::U32 => u32::from_le_bytes(data[off..off + 4].try_into().unwrap()),
            other => bail!(
                "index accessor #{} has unsupported component type {other:?}; expected \
                 UNSIGNED_BYTE, UNSIGNED_SHORT or UNSIGNED_INT",
                accessor.index()
            ),
        };
        out.push(v);
    }
    Ok(out)
}
