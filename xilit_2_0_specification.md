# xilit 2.0 — Specification

## 1. Purpose & Scope

**xilit 2.0** is a Rust command-line tool designed to inspect basic glTF 2.0 assets and export complete multi-material models into the **MDX2** binary format.

Building upon the foundations of Xilit 1.0 (`xilit.md`), Xilit 2.0 drops MDX1 primitive-level export in favor of a model-level export path tailored for low-spec hardware (such as the Mali Bifrost GPU and Allwinner H700/A133p Linux handhelds).

Key constraints and characteristics of Xilit 2.0:

* Targets indexed triangle geometry (`GL_TRIANGLES`).

* Strictly enforces **16-bit indices (`u16`)**: a batch may contain at most 65,536 vertices (index values `0`–`65535`); exceeding that is an unrecoverable error. The number of indices in a batch is not limited to 65,536.

* Bakes world transforms directly into vertex data.

* Groups geometry into material batches using a shared, global vertex format.

## 2. Invocation & Export Commands

The interactive shell and single-command/script execution modes remain identical to Xilit 1.0.

### Model Export Syntax

```
mdx export <object-selector>... <filename.mdx> [options]
```

Unlike primitive-level export, the `mdx export` command aggregates multiple meshes across one or more nodes. Because a single source mesh can be referenced multiple times with distinct world transformations, it may appear multiple times within a material batch since they are batched by material assignment.

#### Options

* `--vfmt <vertexFormat>`: Specifies the vertex format across the entire exported model. Defaults to `position,normal,uv0`.

* `--no-descendants`: Disables automatic recursive inclusion of descendant nodes when selecting a parent node or container.

* `--matmap <mapping>`: Maps custom GLSL uniform names to glTF material paths using a JSON-path style grammar.

## 3. Selectors and Hierarchy Traversal

* **Glob Matching:** Object selectors support standard glob patterns (e.g., `"Chair*"`, `"Table?"`) using standard Rust glob matching.

* **Descendant Inclusion:** By default, selecting a node or object automatically includes all descendant meshes in the glTF hierarchy, with their accumulated world transforms baked into the vertex data. Passing `--no-descendants` restricts export strictly to the explicitly matched node/mesh.

* **Validation:** Selectors matching zero objects produce a non-fatal warning. However, an export resulting in zero geometry is a fatal error.

## 4. Transform and Geometry Processing

* **World Transforms:**

  * **Positions:** Transformed by the accumulated world matrix (`f32`).

  * **Normals:** Transformed by the inverse transpose of the linear 3×3 matrix part and normalized.

  * **Tangents:** Transformed by the linear 3×3 part, made orthogonal to the transformed normal, and normalized.

* **Reflections/Negative Determinants:** If a node's transformation matrix has a negative determinant (reflection/negative scaling), export must fail immediately with an unrecoverable error.

* **Vertex and Index Limits:** Indices are strictly `u16`, so an index can address at most 65,536 vertices (values `0` through `65535`).
  * The limit applies to the **vertex count** of a material batch: if a batch would contain more than 65,536 vertices, export fails with an unrecoverable error.
  * The **index count** of a batch is *not* limited to 65,536. Since vertices can be shared by many triangles, a batch with at most 65,536 vertices may have more than 65,536 indices (bounded only by the `u32` chunk size). Always a multiple of 3, as only `GL_TRIANGLES` is exported.
  * Index values are local to the batch's `VBUF`, so each batch's indices always start at `0` regardless of the number of batches in the model.

## 5. Material Mapping (`--matmap`)

The `--matmap` parameter defines how glTF material properties map to application uniforms.

### Syntax Example

```
--matmap albedo:sampler2D/pbrMetallicRoughness.baseColorTexture.index,normal:sampler2D/normalTexture.index
```

* **Uniform Name:** The target shader uniform name (e.g., `albedo`).

* **Type & Binding:** Formatted as `<type>/<json-path>`.

  * Supported types for now: `float`, `vec2`, `vec3`, `vec4`, `sampler2D`.

  * The path is a dot-notation query into the glTF material structure resolving to a texture or numeric property.

* **Missing Textures:** If an optional texture mapping path resolves to null or is missing from a material, xilit emits a warning and encodes it as a `null` uniform type (`0`).

## 6. MDX2 Binary Container Architecture

MDX2 is a chunked format. All multi-byte numeric values are little-endian.

### Chunk Framing

Every chunk, including the master chunk, follows a standard header:

* **Type (4 bytes):** FourCC identifier.

* **Payload Size (4 bytes):** Length of the payload in bytes (excluding header and 4-byte alignment padding).

The header is followed by the payload, then by zero padding up to the next 4-byte boundary. Because payloads are padded to 4-byte boundaries, every chunk begins at a 4-byte aligned offset relative to the start of its parent's payload.

### The MDX2 Master Chunk

An MDX2 model is a single chunk with the FourCC `MDX2`, the *master chunk*. Its payload consists of all the other chunks (`STRS`, `ADEF`, `VBUF`, `IBUF`, `MATL`, `GEOM`) stored back to back, each with its own header and padding. The master chunk's payload size is therefore the combined size of those chunks, headers and padding included.

A `.mdx` file is exactly one master chunk: it starts with the master chunk's 8-byte header (`MDX2`, payload size), and nothing precedes or follows the chunk. The same chunk can equally be embedded in another container.

### Chunk References & Payload Offsets

A chunk reference is a `u32` byte offset **within the master chunk's payload**: it is measured from the first byte of the payload (the byte immediately following the master chunk's 8-byte header) to the first byte, i.e. the header, of the referenced chunk. The first chunk in the payload is therefore at offset `0`.

References never leave the master chunk's payload and never depend on where the master chunk itself is stored. An MDX2 chunk can thus be embedded in another file, or moved, without touching any of its contents.

Because every chunk begins at a 4-byte aligned offset within the payload, the lower two bits of any chunk reference are always `0`.

## 7. Core MDX2 Chunk Inventory

### `STRS` — Shared String Pool

Contains a sequence of NUL-terminated UTF-8 strings, followed by zero padding to a 4-byte boundary. All attribute names, shader names, texture paths, and uniform names are stored here and referenced by byte offsets relative to the start of the pool payload (string pool offsets, which are not chunk references and need not be 4-byte aligned). Duplicate strings are deduplicated. An MDX2 model contains exactly one `STRS` chunk.

### `ADEF` — Vertex Attribute Definition

The definition starts with a header of two `u32` fields: the number of attributes and the stride (the vertex size in bytes). This is followed by the attribute definitions...

Defines the global vertex layout (matching `--vfmt`). Since the vertex format is global, xilit emits a single `ADEF` chunk that is referenced by every `VBUF`. The definition starts with a header: the number of attributes (`u32`) and the stride, the vertex size in bytes (`u32`). This is followed by the attribute definitions, each definition consisting of five little-endian `u32` fields (20 bytes):

1. Name offset (string pool offset, e.g. of `position`, `normal`, `uv0`)

2. Component count (1, 2, 3, or 4)

3. Type (`GL_FLOAT`, `0x1406`)

4. Normalized (`GL_FALSE`, `0`)

5. Vertex offset (byte offset within each vertex)

Attributes are written in the order given by `--vfmt` and tightly packed (no gaps), so the vertex offset of an attribute is the sum of the sizes (`component count × 4` bytes) of the attributes before it, and the stride written in the header is the sum of all attribute sizes. For the default `position,normal,uv0` format this yields attributes at offsets `0`, `12` and `24` and a stride of `32` bytes.

### `VBUF` — Vertex Buffer

Each `VBUF` chunk contains a header followed by raw interleaved vertex data:

* **Vertex Count:** `u32` number of vertices (at most 65,536, see section 4).

* **ADEF Reference:** `u32` payload offset (see section 6) pointing to the associated `ADEF` chunk.

* **Stride (Vertex Size):** `u32` size of a single vertex in bytes. Identical to the stride in the ADEF header; stored here as well so the buffer size can be computed without the `ADEF`.

* **Vertex Data:** Raw vertex data packed consecutively, containing `vertex count × stride` bytes.

### `IBUF` — Index Buffer

Each `IBUF` chunk contains a header followed by raw index data:

* **Index Count:** `u32` number of indices (a multiple of 3; not limited to 65,536).

* **Primitive Type:** `u32` OpenGL primitive type (`GL_TRIANGLES`).

* **Index Type:** `u32` OpenGL index type (`GL_UNSIGNED_SHORT`).

* **Index Data:** Little-endian `u16` indices, containing `index count × 2` bytes. Every index is less than the vertex count of the batch's `VBUF`.

### `MATL` — Material Definitions

Defines materials used by geometry batches. All fields are stored as little-endian `u32` values:

* **Shader Name:** String pool offset identifying the target shader program.

* **Uniform Count:** `u32` number of uniforms.

* **Uniform Entries:** Each uniform consists of:

  * Uniform name (string pool offset).

  * Type constant (`GL_FLOAT`, `GL_FLOAT_VEC2`, `GL_FLOAT_VEC3`, `GL_FLOAT_VEC4`, `GL_SAMPLER_2D`, or `0` for `NULL`).

  * Size in 32-bit words.

  * **Value payload:**

    * For numeric types: Raw float values.

    * For `GL_SAMPLER_2D`: A single 32-bit word (size 1). The top 4 bits are flags: xilit always writes `0`, meaning the lower 28 bits are a string pool offset of the texture filename/path. All other flag values are reserved for future use.

    * For `NULL_TYPE` (type `0`): Empty value (size `0`).

### `GEOM` — Geometry Batch Descriptions

Defines a renderable batch by referencing its independent data chunks via `u32` payload offsets (see section 6):

* **VBUF Reference:** Payload offset pointing to the dedicated `VBUF` chunk.

* **IBUF Reference:** Payload offset pointing to the dedicated `IBUF` chunk.

* **MATL Reference:** Payload offset pointing to the `MATL` chunk.
