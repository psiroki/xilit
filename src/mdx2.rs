//! MDX2 writer: a master `MDX2` container holding `STRS`, `ADEF`, and, per
//! material batch, `VBUF` / `IBUF` / `MATL` chunks plus a `GEOM` chunk tying
//! them together (spec 2.0 sections 6-7).
//!
//! Every chunk is `FourCC (4) | payload size (u32, unpadded) | payload |
//! zero padding to a multiple of 4`, so every chunk starts 4-byte aligned.
//! The whole output is one master `MDX2` chunk whose payload holds all the
//! others. A chunk reference is the offset of the referenced chunk's header
//! from the first byte of the master chunk's *payload* (so the first child
//! is at offset 0). Nothing in the output depends on where the master chunk
//! is stored: it can be embedded in another file, or moved, unchanged.

use crate::diag::codes;
use crate::vfmt::Layout;
use anyhow::Result;
use std::collections::HashMap;

pub const GL_TRIANGLES: u32 = 0x0004;
pub const GL_UNSIGNED_SHORT: u32 = 0x1403;
pub const GL_FLOAT: u32 = 0x1406;
pub const GL_FALSE: u32 = 0;

/// Size of the master chunk's header (FourCC + payload size); chunk
/// references are measured from the byte after it.
const MASTER_HEADER_SIZE: usize = 8;

/// Shared string pool (`STRS` payload): deduplicated NUL-terminated strings;
/// [`StringPool::intern`] returns the byte offset from the start of the pool.
#[derive(Default)]
pub struct StringPool {
    bytes: Vec<u8>,
    offsets: HashMap<String, u32>,
}

impl StringPool {
    pub fn new() -> StringPool {
        StringPool::default()
    }

    pub fn intern(&mut self, s: &str) -> Result<u32> {
        if let Some(&offset) = self.offsets.get(s) {
            return Ok(offset);
        }
        if s.as_bytes().contains(&0) {
            bail_code!(codes::STRING_HAS_NUL, "string {s:?} contains a NUL byte");
        }
        let offset = u32::try_from(self.bytes.len()).map_err(|_| {
            err_code!(codes::OUTPUT_TOO_LARGE, "string pool exceeds 4 GiB")
        })?;
        self.bytes.extend_from_slice(s.as_bytes());
        self.bytes.push(0);
        self.offsets.insert(s.to_string(), offset);
        Ok(offset)
    }

    #[cfg(test)]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// One renderable batch, ready to be written.
pub struct Batch {
    pub vertex_count: u32,
    /// `vertex_count * layout.vertex_size` bytes of interleaved vertices.
    pub vertex_data: Vec<u8>,
    pub indices: Vec<u16>,
    /// A complete `MATL` payload (see `matmap::build_matl`).
    pub material: Vec<u8>,
}

#[derive(Clone, Copy)]
struct ChunkId(usize);

struct Writer {
    buf: Vec<u8>,
    offsets: Vec<Option<usize>>,
    refs: Vec<(usize, ChunkId)>,
}

impl Writer {
    fn new() -> Writer {
        // Master chunk header; its payload size is patched in `finish`.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"MDX2");
        buf.extend_from_slice(&0u32.to_le_bytes());
        Writer {
            buf,
            offsets: Vec::new(),
            refs: Vec::new(),
        }
    }

    /// Allocate an id so a chunk can be referenced before it is written.
    fn reserve(&mut self) -> ChunkId {
        self.offsets.push(None);
        ChunkId(self.offsets.len() - 1)
    }

    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a chunk reference (a payload offset), resolved in `finish`.
    fn reference(&mut self, id: ChunkId) {
        self.refs.push((self.buf.len(), id));
        self.u32(0);
    }

    fn chunk(&mut self, id: ChunkId, fourcc: &[u8; 4], body: impl FnOnce(&mut Writer)) -> Result<()> {
        self.offsets[id.0] = Some(self.buf.len());
        self.buf.extend_from_slice(fourcc);
        self.u32(0);
        let payload_start = self.buf.len();
        body(self);
        let size = self.buf.len() - payload_start;
        let size32 = u32::try_from(size).map_err(|_| {
            err_code!(codes::OUTPUT_TOO_LARGE, "a chunk payload exceeds 4 GiB")
        })?;
        self.buf[payload_start - 4..payload_start].copy_from_slice(&size32.to_le_bytes());
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>> {
        if u32::try_from(self.buf.len()).is_err() {
            bail_code!(codes::OUTPUT_TOO_LARGE, "output file exceeds 4 GiB");
        }
        for (at, id) in std::mem::take(&mut self.refs) {
            let header = self.offsets[id.0].expect("every referenced chunk is written");
            // Relative to the master payload, not to the start of the file.
            let payload_offset = (header - MASTER_HEADER_SIZE) as u32;
            self.buf[at..at + 4].copy_from_slice(&payload_offset.to_le_bytes());
        }
        let master_payload = (self.buf.len() - MASTER_HEADER_SIZE) as u32;
        self.buf[4..8].copy_from_slice(&master_payload.to_le_bytes());
        Ok(self.buf)
    }
}

/// Serialize a complete MDX2 file. `pool` must already contain the strings
/// used by the batches' `material` payloads; attribute names are added here.
pub fn write(layout: &Layout, mut pool: StringPool, batches: &[Batch]) -> Result<Vec<u8>> {
    // ADEF payload (needs the attribute names interned before STRS is written).
    let defs: Vec<_> = layout.entries.iter().filter(|e| e.emits_attribute).collect();
    let mut adef = Vec::with_capacity(4 + defs.len() * 20);
    adef.extend_from_slice(&(defs.len() as u32).to_le_bytes());
    for entry in &defs {
        let name = pool.intern(&entry.field.name)?;
        for v in [name, entry.component_count, GL_FLOAT, GL_FALSE, entry.offset] {
            adef.extend_from_slice(&v.to_le_bytes());
        }
    }

    let mut w = Writer::new();
    let strs_id = w.reserve();
    let adef_id = w.reserve();
    let ids: Vec<(ChunkId, ChunkId, ChunkId)> =
        batches.iter().map(|_| (w.reserve(), w.reserve(), w.reserve())).collect();

    w.chunk(strs_id, b"STRS", |w| w.buf.extend_from_slice(&pool.bytes))?;
    w.chunk(adef_id, b"ADEF", |w| w.buf.extend_from_slice(&adef))?;

    for (batch, &(vbuf, ibuf, matl)) in batches.iter().zip(&ids) {
        w.chunk(vbuf, b"VBUF", |w| {
            w.u32(batch.vertex_count);
            w.reference(adef_id);
            w.u32(layout.vertex_size);
            w.buf.extend_from_slice(&batch.vertex_data);
        })?;
        w.chunk(ibuf, b"IBUF", |w| {
            w.u32(batch.indices.len() as u32);
            w.u32(GL_TRIANGLES);
            w.u32(GL_UNSIGNED_SHORT);
            for idx in &batch.indices {
                w.buf.extend_from_slice(&idx.to_le_bytes());
            }
        })?;
        w.chunk(matl, b"MATL", |w| w.buf.extend_from_slice(&batch.material))?;
    }

    for &(vbuf, ibuf, matl) in &ids {
        let geom = w.reserve();
        w.chunk(geom, b"GEOM", |w| {
            w.reference(vbuf);
            w.reference(ibuf);
            w.reference(matl);
        })?;
    }

    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfmt::{parse_vfmt, Layout};

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// Walk the master chunk's children: (fourcc, payload offset of the
    /// child's header, payload). Offsets are what chunk references hold.
    fn children(file: &[u8]) -> Vec<(String, usize, &[u8])> {
        assert_eq!(&file[0..4], b"MDX2");
        let master = u32_at(file, 4) as usize;
        assert_eq!(8 + master, file.len(), "master size covers the whole file");
        let payload = &file[8..];
        let mut out = Vec::new();
        let mut at = 0;
        while at < payload.len() {
            assert_eq!(at % 4, 0, "chunk header is 4-byte aligned");
            let size = u32_at(payload, at + 4) as usize;
            let fourcc = String::from_utf8(payload[at..at + 4].to_vec()).unwrap();
            out.push((fourcc, at, &payload[at + 8..at + 8 + size]));
            at += 8 + (size + 3) / 4 * 4;
        }
        assert_eq!(at, payload.len());
        out
    }

    fn sample() -> Vec<u8> {
        let layout = Layout::build(parse_vfmt("position,_/pad2,extra/vec2").unwrap()).unwrap();
        assert_eq!(layout.vertex_size, 22); // not a multiple of 4 on purpose
        let mut pool = StringPool::new();
        let shader = pool.intern("lit").unwrap();
        let mut material = Vec::new();
        material.extend_from_slice(&shader.to_le_bytes());
        material.extend_from_slice(&0u32.to_le_bytes());
        let batch = Batch {
            vertex_count: 3,
            vertex_data: (0..66).collect(),
            indices: vec![0, 1, 2],
            material,
        };
        write(&layout, pool, &[batch]).unwrap()
    }

    #[test]
    fn container_structure_and_alignment() {
        let file = sample();
        assert_eq!(file.len() % 4, 0);
        let kinds: Vec<String> = children(&file).into_iter().map(|c| c.0).collect();
        assert_eq!(kinds, ["STRS", "ADEF", "VBUF", "IBUF", "MATL", "GEOM"]);
    }

    #[test]
    fn strings_are_deduplicated_and_adef_is_correct() {
        let file = sample();
        let c = children(&file);
        // "lit", then the emitted attribute names; hidden `_` entries add nothing.
        assert_eq!(c[0].2, b"lit\0position\0extra\0");
        let adef = c[1].2;
        assert_eq!(u32_at(adef, 0), 2);
        // position: name@4, 3 comps, GL_FLOAT, not normalized, offset 0
        assert_eq!(
            [u32_at(adef, 4), u32_at(adef, 8), u32_at(adef, 12), u32_at(adef, 16), u32_at(adef, 20)],
            [4, 3, GL_FLOAT, 0, 0]
        );
        // extra: after 12 bytes of position + 2 of padding
        assert_eq!(
            [u32_at(adef, 24), u32_at(adef, 28), u32_at(adef, 32), u32_at(adef, 36), u32_at(adef, 40)],
            [13, 2, GL_FLOAT, 0, 14]
        );
    }

    #[test]
    fn buffers_and_padding() {
        let file = sample();
        let c = children(&file);
        let vbuf = c[2].2;
        assert_eq!(u32_at(vbuf, 0), 3);
        assert_eq!(&file[8 + c[1].1..][..4], b"ADEF");
        assert_eq!(u32_at(vbuf, 4) as usize, c[1].1, "VBUF points at the ADEF header");
        assert_eq!(u32_at(vbuf, 8), 22);
        assert_eq!(vbuf.len(), 12 + 66);
        let ibuf = c[3].2;
        assert_eq!(
            [u32_at(ibuf, 0), u32_at(ibuf, 4), u32_at(ibuf, 8)],
            [3, GL_TRIANGLES, GL_UNSIGNED_SHORT]
        );
        assert_eq!(ibuf.len(), 12 + 6, "payload size excludes padding");
    }

    #[test]
    fn geom_references_resolve_to_the_right_chunks() {
        let file = sample();
        let c = children(&file);
        let geom = c[5].2;
        let target = |i: usize| &file[8 + u32_at(geom, i * 4) as usize..][..4];
        assert_eq!(target(0), b"VBUF");
        assert_eq!(target(1), b"IBUF");
        assert_eq!(target(2), b"MATL");
        assert_eq!(u32_at(geom, 0) as usize, c[2].1);
    }

    #[test]
    fn nul_in_strings_is_rejected() {
        let e = StringPool::new().intern("a\0b").unwrap_err();
        assert_eq!(crate::diag::code_of(&e), codes::STRING_HAS_NUL);
    }

    #[test]
    fn intern_returns_stable_offsets() {
        let mut p = StringPool::new();
        assert_eq!(p.intern("").unwrap(), 0);
        assert_eq!(p.intern("ab").unwrap(), 1);
        assert_eq!(p.intern("").unwrap(), 0);
        assert_eq!(p.intern("ab").unwrap(), 1);
        assert_eq!(p.bytes(), b"\0ab\0");
    }

    #[test]
    fn first_chunk_is_at_payload_offset_zero() {
        let file = sample();
        let c = children(&file);
        assert_eq!((c[0].0.as_str(), c[0].1), ("STRS", 0));
        // the ADEF follows the padded STRS (header + payload rounded up to 4)
        assert_eq!(c[1].1, 8 + (c[0].2.len() + 3) / 4 * 4);
    }

    #[test]
    fn references_are_payload_relative_so_the_chunk_can_be_embedded() {
        let file = sample();
        // Embed the unchanged chunk at a 4-byte aligned offset inside a bigger
        // container; resolving references against *its* payload still works.
        let mut host = vec![0xAAu8; 24];
        host.extend_from_slice(&file);
        host.extend_from_slice(&[0xBB; 8]);
        let chunk = &host[24..24 + file.len()];
        assert_eq!(chunk, &file[..], "the embedded bytes are untouched");

        let payload = &chunk[8..];
        let geom_at = children(chunk).iter().find(|c| c.0 == "GEOM").unwrap().1;
        let vbuf_ref = u32_at(payload, geom_at + 8) as usize;
        assert_eq!(&payload[vbuf_ref..][..4], b"VBUF");
        let adef_ref = u32_at(payload, vbuf_ref + 8 + 4) as usize;
        assert_eq!(&payload[adef_ref..][..4], b"ADEF");
        // and they are not file offsets: that would point 8 bytes further on
        assert_ne!(&chunk[vbuf_ref..][..4], b"VBUF");
    }
}
