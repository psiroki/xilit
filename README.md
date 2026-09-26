# xilit

CLI tool for inspecting glTF 2.0 / `.glb` models and exporting a single mesh
primitive to **MDX1**, a minimal fixed-layout vertex/index binary format.
Spec: `xilit.md`.

## Usage

```sh
xilit model.glb                 # interactive shell
xilit model.glb -c 'mesh list'  # run command(s), exit
xilit model.glb -s script.xlt   # run commands from a file
```

Commands: `list`, `mesh|image|material|texture|node|scene list`, `mesh export`.
Objects are addressed by `"Name"` or `#index`. `-c`/`-s` abort on first error;
the interactive shell prints the error and keeps going.

## Mesh export

```
mesh export <mesh>/<primitiveIndex> <file.mdx> [--vfmt <format>]
```

Only `TRIANGLES`, indexed primitives are supported. Indices are converted to
`u16` (error if any index doesn't fit). Default `--vfmt`:
`position,normal,uv0`.

**Vertex format language:** `<name>/<binding>[,...]`, binding = attr name if
omitted.

| binding | meaning |
|---|---|
| `position`/`normal`/`tangent`/`uvN` | imported glTF attribute (float only, no conversion) |
| `float`/`vec2`/`vec3`/`vec4` | app-filled, zero-initialized |
| `padN` | N bytes of padding (must be named `_`) |

Name `_` = no MDX1 attribute definition emitted (still occupies vertex bytes).
Fields are packed consecutively, in order, with no
implicit alignment — use explicit `_/padN` if you need it. Each source
binding may appear at most once. Bad-looking GLSL identifiers only warn
(exit 0); everything else in §22 of the spec is a hard error.

## MDX1 layout

```
header (32B, 8×u32 LE: fourCC, vertexCount, indexCount, attrCount,
        vertexSize, GL_TRIANGLES, GL_UNSIGNED_SHORT, stringPoolSize)
string pool   (unique emitted names, NUL-terminated, padded to 4B)
attr defs     (attrCount × 5×u32 LE: nameOffset, componentCount,
               GL_FLOAT, GL_FALSE, vertexOffset)
vertex buffer (vertexCount × vertexSize bytes, f32 LE)
index buffer  (indexCount × u16 LE)
```

`stringPoolSize` includes the 4-byte alignment padding — that padding exists
purely so the file can be `mmap`'d/read whole and indexed without re-copying;
attribute `vertexOffset`s are unaffected by it.

## Building

```sh
cargo build --release
cargo test
```

Requires Rust 2024. `gltf` is used with `default-features = false` (no
`import`/`image`) — buffer loading (GLB bin chunk, external files, base64
data URIs) is hand-rolled in `gltf_util.rs` since xilit never decodes pixel
data, only reports image/texture metadata.

## Layout

```
src/
  main.rs        entry point, mode dispatch
  cli.rs         argv parsing
  shell.rs       tokenize + dispatch (interactive/-c/-s)
  objref.rs      "Name"/#index + quoting
  vfmt.rs        --vfmt parser + layout computation
  gltf_util.rs   glTF/GLB + buffer loading, accessor readers
  mdx.rs         MDX1 writer
  commands/      list.rs, mesh.rs
```
