# MDX 2.0 File Format Specification

This document defines the binary layout and parsing specifications for the MDX 2.0 file format. The format is designed for lightweight, low-overhead rendering engines targeting resource-constrained hardware (such as mobile GPUs and low-spec Linux handhelds like the Allwinner H700 or A133p).

## 1. General Architecture & Byte Ordering

An MDX 2.0 model is a single **master chunk** with the FourCC `MDX2`; its payload contains all other chunks, stored back to back. A `.mdx` file is exactly one master chunk, with nothing before or after it. Every chunk, the master chunk included, begins with an 8-byte header consisting of:

1. **FourCC Magic Constant** (4 bytes): An ASCII byte sequence identifying the chunk type.

2. **Chunk Size** (`u32`, 4 bytes): A 32-bit unsigned integer representing the size of the chunk data payload in bytes (excluding the 8-byte chunk header itself). Note that padding bytes are added after the chunk payload as necessary to maintain 4-byte alignment, but these padding bytes are **not** counted in the chunk size.

Because all chunks and payloads are aligned to 4-byte boundaries, all chunk offsets, sizes, and chunk references are guaranteed to be 4-byte aligned (meaning their lower two bits are always `0`).

### Master Chunk and Chunk References

The master chunk's payload size is the combined size of its child chunks, headers and padding included.

A **chunk reference** is a `u32` byte offset *within the master chunk's payload*: it is measured from the first byte of the payload (the byte immediately following the master chunk's 8-byte header) to the first byte, i.e. the header, of the referenced chunk. The first child chunk is therefore at offset `0`. References never leave the master chunk's payload and never depend on where the master chunk itself is stored, so an MDX 2.0 chunk can be embedded in another file or moved without modifying any of its contents.

### Endianness

**All multi-byte numeric values** (including 32-bit unsigned integers (`u32`), 16-bit unsigned integers (`u16`), 32-bit floating-point values (`float`), chunk references, chunk sizes, and counts) are stored in **little-endian** byte order.

### Recognized FourCC Constants

* `MDX2` (`0x3258444d`): Master Chunk

* `STRS` (`0x53525453`): String Pool Chunk

* `ADEF` (`0x46454441`): Vertex Attribute Definition Chunk

* `VBUF` (`0x46554256`): Vertex Buffer Chunk

* `IBUF` (`0x46554249`): Index Buffer Chunk

* `MATL` (`0x4c54414d`): Material Chunk

* `GEOM` (`0x4d4f4547`): Geometry Batch Chunk

## 2. String Pool (`STRS`)

The `STRS` chunk contains a contiguous block of null-terminated UTF-8 strings (`char` arrays ending with `\x00`).

* **Structure:** Raw byte payload following the chunk header.

* **Resolution:** All internal string references within other chunks (such as shader names, uniform names, or texture file paths) are expressed as absolute little-endian byte offsets relative to the start of the string pool data payload.

## 3. Vertex Attribute Definition (`ADEF`)

The `ADEF` chunk defines the layout and structure of vertex attributes for vertex buffers.

* **Structure:** Layout definition detailing attribute semantics, component counts, data types, and byte strides (configured for standard position, normal, and UV coordinates).

* **Reusability:** Multiple `VBUF` / `GEOM` blocks may reference a single `ADEF` definition if they share an identical vertex layout.

## 4. Vertex Buffer (`VBUF`)

The `VBUF` chunk contains vertex attribute data paired with its structural layout and sizing metadata.

### Header Layout (All little-endian)

* `vertex_count` (`u32`): Total number of vertices in the buffer.

* `adef_offset` (`u32`): 4-byte aligned payload offset (chunk reference) pointing to the associated `ADEF` chunk.

* `stride` (`u32`): Total size of a single vertex in bytes.

### Data Payload

* Followed immediately by `vertex_count * stride` bytes of raw vertex attribute data.

## 5. Index Buffer (`IBUF`)

The `IBUF` chunk contains primitive connectivity indices. MDX 2.0 prioritizes 16-bit indices for hardware compatibility.

### Header Layout (All little-endian)

* `index_count` (`u32`): Total number of indices.

* `primitive_type` (`u32`): Primitive topology type. Currently, only `GL_TRIANGLES` (`0x0004`) is supported.

* `index_type` (`u32`): Index data type. Currently, only `GL_UNSIGNED_SHORT` (`0x1403`, 16-bit indices) is supported.

### Data Payload

* Followed immediately by `index_count * sizeof(uint16_t)` bytes of raw little-endian index data. Exceeding the 65,535 vertex/index limit for a single 16-bit index buffer is treated as an unrecoverable structural error.

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

4. `value`: Packed little-endian data payload whose size is `value_size * 4` bytes:

| Type Constant | Hex Value | Value Payload Size & Format | 
| ----- | ----- | ----- | 
| `NULL_TYPE` | `0x0000` | Size 0 (no payload). Used for missing or unpopulated slots (e.g., optional texture maps). | 
| `GL_FLOAT` | `0x1406` | 1 $\times$ `float` (4 bytes, little-endian) | 
| `GL_FLOAT_VEC2` | `0x8B50` | 2 $\times$ `float` (8 bytes, little-endian) | 
| `GL_FLOAT_VEC3` | `0x8B51` | 3 $\times$ `float` (12 bytes, little-endian) | 
| `GL_FLOAT_VEC4` | `0x8B52` | 4 $\times$ `float` (16 bytes, little-endian) | 
| `GL_SAMPLER_2D` | `0x8B5E` | 1 $\times$ `u32` reference word (4 bytes, little-endian). The upper 4 bits act as a flags bitmask (where `0` denotes an internal texture reference / string pool path index), followed by the lower 28 bits representing the string pool offset of the texture path string. | 

## 7. Geometry Batch (`GEOM`)

The `GEOM` chunk binds together a discrete renderable mesh batch by referencing its underlying buffers and material via 4-byte aligned payload offsets (chunk references).

### Header Layout (All little-endian)

* `vbuf_offset` (`u32`): 4-byte aligned payload offset pointing to the `VBUF` chunk.

* `ibuf_offset` (`u32`): 4-byte aligned payload offset pointing to the `IBUF` chunk.

* `matl_offset` (`u32`): 4-byte aligned payload offset pointing to the `MATL` chunk.

Readers can read the master chunk header, iterate the chunks in its payload by reading chunk headers and sizes, parse individual `GEOM` entries, resolve their payload offsets (relative to the start of the master chunk's payload) to the respective chunks, and submit draw calls efficiently.