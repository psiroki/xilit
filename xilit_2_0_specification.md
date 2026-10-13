# xilit 2.0 — Specification

## 1. Purpose & Scope

**xilit 2.0** is a Rust command-line tool designed to inspect basic glTF 2.0 assets and export complete multi-material models into the **MDX2** binary format.

Building upon the foundations of Xilit 1.0 (`xilit.md`), Xilit 2.0 drops MDX1 primitive-level export in favor of a model-level export path tailored for low-spec hardware (such as the Mali Bifrost GPU and Allwinner H700/A133p Linux handhelds).

Key constraints and characteristics of Xilit 2.0:

* Targets indexed triangle geometry (`GL_TRIANGLES`).

* Strictly enforces **16-bit indices (`u16`)**; exceeding 65,535 vertices/indices in a batch is an unrecoverable error.

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

* **Index Limits:** Indices are strictly `u16`. If a combined primitive batch exceeds 65,535 indices, export fails with an unrecoverable error.

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

MDX2 uses a chunked master container structure. All multi-byte numeric values are little-endian.

### Chunk Framing

Every chunk (including the master container) follows a standard header:

* **Type (4 bytes):** FourCC identifier.

* **Payload Size (4 bytes):** Length of the payload in bytes (excluding header and 4-byte alignment padding).

Because payloads are padded to 4-byte boundaries, every subsequent chunk naturally begins at a 4-byte aligned offset.

### Chunk References & File Offsets

References between chunks use absolute **file byte offsets** (or offsets relative to the start of the master container). Because all chunk payloads and headers are padded to 4-byte boundaries, every chunk begins at a 4-byte aligned offset, meaning the lower two bits of any chunk reference offset are always `0`.

## 7. Core MDX2 Chunk Inventory

### `STRS` — Shared String Pool

Contains a sequence of NUL-terminated UTF-8 strings padded to a 4-byte boundary. All attribute names, shader names, texture paths, and uniform names are stored here as byte offsets relative to the start of the pool. Duplicate strings are deduplicated.

### `ADEF` — Vertex Attribute Definition

Defines the global vertex layout (matching `--vfmt`). The definition starts with a header: the number of attributes (`u32`). This is followed by the attribute definitions, each definition consisting of little-endian `u32` fields:

1. Name offset (string pool offset)

2. Component count (1, 2, 3, or 4)

3. Type (`GL_FLOAT`)

4. Normalized (`GL_FALSE`)

5. Vertex offset (byte offset within each vertex)

### `VBUF` — Vertex Buffer

Each `VBUF` chunk contains a header followed by raw interleaved vertex data:

* **Vertex Count:** `u32` number of vertices.

* **ADEF Reference:** 4-byte file offset pointing to the associated `ADEF` chunk.

* **Stride (Vertex Size):** `u32` size of a single vertex in bytes.

* **Vertex Data:** Raw vertex data packed consecutively, containing `vertex count × stride` bytes.

### `IBUF` — Index Buffer

Each `IBUF` chunk contains a header followed by raw index data:

* **Index Count:** `u32` number of indices.

* **Primitive Type:** `u32` OpenGL primitive type (`GL_TRIANGLES`).

* **Index Type:** `u32` OpenGL index type (`GL_UNSIGNED_SHORT`).

* **Index Data:** Little-endian `u16` indices, containing `index count × 2` bytes.

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

    * For `GL_SAMPLER_2D`: A 32-bit word where the top 4 bits reserve flags (`0` for string pool offset containing the texture filename/path).

    * For `GL_NULL` (type `0`): Empty value (size `0`).

### `GEOM` — Geometry Batch Descriptions

Defines a renderable batch by referencing its independent data chunks via 4-byte file offsets:

* **VBUF Reference:** File offset pointing to the dedicated `VBUF` chunk.

* **IBUF Reference:** File offset pointing to the dedicated `IBUF` chunk.

* **MATL Reference:** File offset pointing to the `MATL` chunk.