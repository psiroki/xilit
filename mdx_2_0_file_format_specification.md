# MDX 2.0 File Format Specification

This document defines the binary layout and parsing rules for the MDX 2.0 file format. The format is designed for lightweight, low-overhead rendering engines targeting resource-constrained hardware (such as mobile GPUs and low-spec Linux handhelds like the Allwinner H700 or A133p).

## 1. General Architecture & Byte Ordering

An MDX 2.0 model is a single **master chunk** with the FourCC `MDX2`; its payload contains all other chunks, stored back to back. A `.mdx` file is exactly one master chunk, with nothing before or after it.

Every chunk, the master chunk included, begins with an 8-byte header:

| Field | Type | Description |
| ----- | ---- | ----------- |
| FourCC | 4 bytes | ASCII byte sequence identifying the chunk type. |
| Chunk Size | `u32` | Size of the chunk payload in bytes, excluding the 8-byte header and excluding any trailing padding. |

The payload follows the header. After the payload, zero bytes are added as necessary to reach the next 4-byte boundary. These padding bytes are **not** counted in the chunk size.

Because every chunk starts at a 4-byte aligned offset within its parent's payload, all chunk offsets and chunk references are 4-byte aligned (their lower two bits are always `0`).

### Master Chunk and Chunk References

The master chunk's payload size is the combined size of its child chunks, headers and padding included.

A **chunk reference** is a `u32` byte offset *within the master chunk's payload*: it is measured from the first byte of the payload (the byte immediately following the master chunk's 8-byte header) to the first byte, i.e. the header, of the referenced chunk. The first child chunk is therefore at offset `0`. References never leave the master chunk's payload and never depend on where the master chunk itself is stored, so an MDX 2.0 chunk can be embedded in another file or moved without modifying any of its contents.

References may point forward or backward; the order of child chunks is not significant. A reference must point to a chunk of the expected type (e.g. `adef_offset` must point to an `ADEF` chunk).

### Endianness

**All multi-byte numeric values** (including `u32`, `u16`, `float`, chunk references, chunk sizes, and counts) are stored in **little-endian** byte order.

### Recognized FourCC Constants

| FourCC | Value (as little-endian `u32`) | Chunk |
| ------ | ------------------------------ | ----- |
| `MDX2` | `0x3258444d` | Master Chunk |
| `STRS` | `0x53525453` | String Pool Chunk |
| `ADEF` | `0x46454441` | Vertex Attribute Definition Chunk |
| `VBUF` | `0x46554256` | Vertex Buffer Chunk |
| `IBUF` | `0x46554249` | Index Buffer Chunk |
| `MATL` | `0x4c54414d` | Material Chunk |
| `GEOM` | `0x4d4f4547` | Geometry Batch Chunk |

### Chunk Relationships

```
GEOM ──► VBUF ──► ADEF ──► (names in STRS)
  │
  ├────► IBUF
  └────► MATL ──► (names, shader, texture paths in STRS)
```

A model contains exactly one `STRS` chunk. It may contain any number of `ADEF`, `VBUF`, `IBUF`, `MATL`, and `GEOM` chunks. A single chunk may be referenced by several other chunks (for example, one `ADEF` shared by every `VBUF`, or one `MATL` shared by several `GEOM` batches).

## 2. String Pool (`STRS`)

The `STRS` chunk contains a contiguous block of null-terminated UTF-8 strings (`char` arrays ending with `\x00`).

* **Structure:** Raw byte payload following the chunk header. The payload may be followed by padding to a 4-byte boundary as for any chunk.
* **Resolution:** All string references within other chunks (attribute names, shader names, uniform names, texture paths) are **string pool offsets**: absolute little-endian byte offsets relative to the start of the `STRS` payload. They are *not* chunk references, so they are not necessarily 4-byte aligned.
* **Deduplication:** Writers should store each distinct string once; several references may point to the same string. Readers must not assume that strings are stored in any particular order.

## 3. Vertex Attribute Definition (`ADEF`)

The `ADEF` chunk describes the layout of a single interleaved vertex, including its total size, so that a renderer can bind a raw vertex buffer to shader attributes without needing any other information from the `VBUF` chunk.

### Header Layout (All little-endian)

* `attribute_count` (`u32`): Number of attribute definitions following the header.
* `stride` (`u32`): Size of one vertex in bytes. Every attribute must lie within it (see *Rules*).


### Attribute Entry Structure

The header is followed by `attribute_count` entries of exactly five `u32` fields each (20 bytes per entry):

| # | Field | Type | Description |
| - | ----- | ---- | ----------- |
| 1 | `name_offset` | `u32` | String pool offset of the attribute name (e.g. `position`, `normal`, `uv0`). |
| 2 | `component_count` | `u32` | Number of components of the attribute: `1`, `2`, `3` or `4`. |
| 3 | `type` | `u32` | OpenGL component data type. Currently only `GL_FLOAT` (`0x1406`) is supported. |
| 4 | `normalized` | `u32` | OpenGL normalization flag. Currently always `GL_FALSE` (`0`). |
| 5 | `vertex_offset` | `u32` | Byte offset of the attribute from the start of a vertex. |

The payload size of an `ADEF` chunk is therefore `8 + attribute_count * 20` bytes, which is always a multiple of 4, so no padding is required.

### Rules

* An attribute occupies `component_count * 4` bytes (all components are 32-bit `float`s) starting at `vertex_offset`.
* Attributes must not overlap, and every attribute must lie entirely within the vertex, i.e. `vertex_offset + component_count * 4 <= stride`. The `stride` may exceed the end of the last attribute if the layout contains padding.
* Attributes may be listed in any order; the order of entries need not match the order of `vertex_offset` values.
* The `stride` is stored redundantly in both the `ADEF` and the `VBUF` chunk, so that each is self-contained: the `ADEF` fully describes a vertex without consulting any `VBUF`, and a `VBUF` can compute the size of its data without consulting the `ADEF`. The two values must be equal: the `stride` of a `VBUF` must match the `stride` of the `ADEF` it references.
* **Reusability:** Multiple `VBUF` chunks may reference a single `ADEF` chunk if they share the same vertex layout (and therefore the same stride). Writers producing a model with one global vertex format typically emit a single `ADEF` chunk referenced by all `VBUF` chunks.

### Example

The default layout `position,normal,uv0` (tightly packed): `attribute_count = 3`, `stride = 32`, with the entries:

| Attribute | `name_offset` → | `component_count` | `type` | `normalized` | `vertex_offset` |
| --------- | --------------- | ----------------- | ------ | ------------ | --------------- |
| 0 | `"position"` | 3 | `0x1406` | 0 | 0 |
| 1 | `"normal"` | 3 | `0x1406` | 0 | 12 |
| 2 | `"uv0"` | 2 | `0x1406` | 0 | 24 |

## 4. Vertex Buffer (`VBUF`)

The `VBUF` chunk contains vertex attribute data paired with its structural layout and sizing metadata.

### Header Layout (All little-endian)

* `vertex_count` (`u32`): Total number of vertices in the buffer.
* `adef_offset` (`u32`): Chunk reference pointing to the associated `ADEF` chunk.
* `stride` (`u32`): Total size of a single vertex in bytes. Must equal the `stride` of the `ADEF` chunk referenced by `adef_offset`. It is stored here as well so that the size of the vertex data can be computed without resolving the `ADEF`.

### Data Payload

* Followed immediately by `vertex_count * stride` bytes of raw interleaved vertex attribute data, laid out as described by the referenced `ADEF` chunk.
* The payload size of the chunk is `12 + vertex_count * stride` bytes.
* When the `VBUF` is used with an `IBUF` of type `GL_UNSIGNED_SHORT`, `vertex_count` must not exceed 65,536 (see *Vertex and Index Limits* in section 5).

## 5. Index Buffer (`IBUF`)

The `IBUF` chunk contains primitive connectivity indices. MDX 2.0 prioritizes 16-bit indices for hardware compatibility.

### Header Layout (All little-endian)

* `index_count` (`u32`): Total number of indices.
* `primitive_type` (`u32`): Primitive topology type. Currently, only `GL_TRIANGLES` (`0x0004`) is supported. For `GL_TRIANGLES`, `index_count` must be a multiple of 3 (other primitive types, should they be added, define their own constraint).
* `index_type` (`u32`): Index data type. Currently, only `GL_UNSIGNED_SHORT` (`0x1403`, 16-bit indices) is supported.

### Data Payload

* Followed immediately by `index_count * sizeof(uint16_t)` bytes of raw little-endian index data. The payload size of the chunk is `12 + index_count * 2` bytes; if `index_count` is odd, two zero padding bytes follow to restore 4-byte alignment (not counted in the chunk size).
* Each index refers to a vertex of the `VBUF` used together with this `IBUF` in a `GEOM` chunk, and must be less than that `VBUF`'s `vertex_count`.
* For `GL_UNSIGNED_SHORT`, every index value must be in the range `0` to `65535`, and must be less than the `vertex_count` of the `VBUF` it is used with (see the limits below).

### Vertex and Index Limits (16-bit indices)

Because `GL_UNSIGNED_SHORT` indices are 16 bits wide, an index can address at most 65,536 distinct vertices (index values `0` through `65535`). The limits are therefore:

| Quantity | Limit when `index_type` is `GL_UNSIGNED_SHORT` |
| -------- | ---------------------------------------------- |
| Vertices per `VBUF` used with the `IBUF` (`vertex_count`) | **at most 65,536** |
| Value of any single index | at most 65,535 (and less than `vertex_count`) |
| Number of entries in an `IBUF` (`index_count`) | **not limited to 65,536**; may be larger (bounded only by the `u32` chunk size, and by `GL_TRIANGLES` requiring a multiple of 3) |

The limit applies to the vertex buffer, not to the index buffer: indices may reference the same vertices many times, so an index buffer can legitimately hold more than 65,536 entries as long as every index value fits in 16 bits and refers to an existing vertex.

A `GEOM` chunk that combines an `IBUF` of type `GL_UNSIGNED_SHORT` with a `VBUF` whose `vertex_count` exceeds 65,536 is an unrecoverable structural error. A model that needs more vertices must be split into several `GEOM` batches, each with its own `VBUF` and `IBUF`.

## 6. Material (`MATL`)

The `MATL` chunk defines rendering shader bindings and associated uniform parameters. All fields within the `MATL` header and uniform list are stored as little-endian 32-bit unsigned integers (`u32`) or corresponding typed values.

### Header Layout (All little-endian)

* `shader_name_offset` (`u32`): String pool offset pointing to the target shader identifier string.
* `uniform_count` (`u32`): Number of uniform parameter entries following the header.

### Uniform Entry Structure

For each uniform up to `uniform_count`, the file contains:

1. `uniform_name_offset` (`u32`): String pool offset pointing to the uniform name.
2. `type` (`u32`): OpenGL uniform type constant.
3. `value_size` (`u32`): Size of the value payload in 32-bit words.
4. `value`: Packed little-endian data payload whose size is `value_size * 4` bytes.

Since all fields are 32-bit, every uniform entry is 4-byte aligned. Uniform entries are stored consecutively, each taking `12 + value_size * 4` bytes.

| Type Constant | Hex Value | `value_size` | Value Payload Format |
| ----- | ----- | ----- | ----- |
| `NULL_TYPE` | `0x0000` | 0 | No payload. Used for missing or unpopulated slots (e.g., optional texture maps). |
| `GL_FLOAT` | `0x1406` | 1 | 1 × `float` (4 bytes) |
| `GL_FLOAT_VEC2` | `0x8B50` | 2 | 2 × `float` (8 bytes) |
| `GL_FLOAT_VEC3` | `0x8B51` | 3 | 3 × `float` (12 bytes) |
| `GL_FLOAT_VEC4` | `0x8B52` | 4 | 4 × `float` (16 bytes) |
| `GL_SAMPLER_2D` | `0x8B5E` | 1 | 1 × `u32` reference word (see below). |

### `GL_SAMPLER_2D` Reference Word

The value of a `GL_SAMPLER_2D` uniform is a single `u32` reference word, split into two bit fields:

| Bits | Field | Description |
| ---- | ----- | ----------- |
| 31–28 (upper 4 bits) | flags | Must be `0`, meaning the lower 28 bits are a string pool offset of the texture path. **All other values are reserved for future use.** |
| 27–0 (lower 28 bits) | string pool offset | Offset of the texture path string in the `STRS` chunk. |

* Writers **must** write `0` in the upper 4 bits.
* Readers should treat a reference word with non-zero flags as unsupported (reserved for future use) rather than interpreting its lower bits as a string pool offset.
* The extraction of the string pool offset is therefore `word & 0x0FFFFFFF`, and the flags are `word >> 28`.

## 7. Geometry Batch (`GEOM`)

The `GEOM` chunk binds together a discrete renderable mesh batch by referencing its underlying buffers and material via chunk references.

### Header Layout (All little-endian)

* `vbuf_offset` (`u32`): Chunk reference pointing to the `VBUF` chunk.
* `ibuf_offset` (`u32`): Chunk reference pointing to the `IBUF` chunk.
* `matl_offset` (`u32`): Chunk reference pointing to the `MATL` chunk.

The payload size of a `GEOM` chunk is 12 bytes.

## 8. Reading an MDX 2.0 File

Readers can proceed as follows:

1. Read the 8-byte master chunk header and verify the FourCC is `MDX2`. The master chunk's payload must be exactly as large as the remainder of the file.
2. Iterate the chunks in the master payload: read each chunk header, record its type and payload, and advance by `8 + size`, rounded up to a multiple of 4.
3. Locate the `STRS` chunk, which is required to resolve every string pool offset.
4. For each `GEOM` entry, resolve `vbuf_offset`, `ibuf_offset` and `matl_offset` (relative to the start of the master chunk's payload) to the respective chunks, resolve the `ADEF` chunk referenced by the `VBUF`, and submit draw calls.

Readers should reject a file as malformed if a chunk extends beyond its parent's payload, a reference does not point to the header of a chunk of the expected type, a string pool offset points outside the string pool (or to a string without a terminating `\x00`), an index is greater than or equal to the `vertex_count` of its `VBUF`, a `VBUF` used with `GL_UNSIGNED_SHORT` indices has more than 65,536 vertices, or any of the other limits stated above is exceeded.
