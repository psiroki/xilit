# xilit

CLI tool for inspecting glTF 2.0 / `.glb` models and exporting them to **MDX2**,
a chunked binary model format for low-spec GPUs (Mali Bifrost, Allwinner
H700/A133p handhelds): u16 indexed triangles, world transforms baked into the
vertices, one batch per material, one global vertex layout.

Anything xilit can't export correctly is an error, not a silent approximation.
Specs: `xilit_2_0_specification.md` (this version), `xilit.md` (1.0, MDX1,
superseded: `mesh export` no longer exists).

## Usage

```sh
xilit model.glb                       # interactive shell
xilit model.glb -c 'node list'        # run command(s), exit
xilit model.glb -s script.xlt         # run commands from a file
xilit model.glb --lenient -c '...'    # load files that fail glTF validation
```

Commands are separated by `;` or newlines (not inside quoted strings). `-c`/`-s`
stop at the first error (exit status 1); the interactive shell prints it and
carries on. Names containing spaces or `/` must be JSON-quoted: `"My Mesh"`.

| command | |
|---|---|
| `list` | object counts |
| `mesh list` `node list` `scene list` `material list` `texture list` `image list` | one line per object (`#index: "name" ...`) |
| `mdx export <selector>... <file.mdx> [options]` | export (below) |

`--lenient` skips the `gltf` crate's validation (needed e.g. for
`EXT_texture_webp`, which hides `texture.source` in an extension), printing what
it ignored as a warning. Malformed data can then panic instead of erroring
cleanly; fine for trusted pipeline input.

## `mdx export`

```sh
xilit scene.glb -c 'mdx export Root "Chair*" out.mdx --vfmt position,normal,uv0 \
  --matmap albedo:sampler2D/pbrMetallicRoughness.baseColorTexture.index'
```

| option | |
|---|---|
| `--vfmt <fmt>` | vertex format for the whole model, default `position,normal,uv0` |
| `--matmap <map>` | glTF material path → shader uniform mapping, default none |
| `--no-descendants` | export only the matched nodes, not their subtrees |

### Selectors

Everything resolves to a set of **nodes**; each is exported once however many
selectors hit it. Names are [glob](https://docs.rs/glob) patterns (`*`, `?`, `[a-c]`).

| selector | selects |
|---|---|
| `Chair*` | nodes, meshes **and** scenes with matching names |
| `node:<ref>` | nodes only |
| `mesh:<ref>` | every node instancing a matching mesh (the same mesh can appear many times, each with its own transform) |
| `scene:<ref>` | the scene's root nodes |
| `#3` | node 3 (`mesh:#3`, `scene:#0` for the others) |

Selected nodes include their descendants unless `--no-descendants`. A selector
matching nothing warns; an export with no geometry is an error.

### Geometry rules

- Triangle-list, **indexed** primitives only; vertex data must be `f32` (no conversion).
- World matrix baked in: positions by the matrix, normals by the inverse
  transpose (normalized), tangents by the linear part, orthogonalized against
  the new normal, normalized (`w` kept). Requesting `tangent` needs `NORMAL`.
- Negative-determinant (mirroring) world transforms are an error, as are
  degenerate ones. Skins and morph targets are ignored with a warning.
- Primitives are merged per material (`None` = default material, last).
  **A batch may not exceed 65,535 vertices or 65,535 indices.**
- Vertices are copied as-is (no dedup/remap), instance after instance.

### Vertex format

`<attribute>/<binding>[,...]`; the binding defaults to the attribute name.
Fields are packed in order with **no implicit alignment**.

| binding | meaning |
|---|---|
| `position` `normal` `tangent` `uv0` `uv1`... | glTF attribute (3/3/4/2 floats), each at most once |
| `float` `vec2` `vec3` `vec4` | zero-filled, for the application to write |
| `padN` | N zero bytes, name must be `_` |

The name `_` reserves the space but emits no MDX attribute: `_/position`,
`_/vec4`, `_/pad4`. Non-GLSL-looking names only warn.

### Material mapping

`--matmap name:type/path[,...]`, with `type` one of `float vec2 vec3 vec4
sampler2D`. `path` is dot-notation into the glTF material **with schema defaults
filled in** (`pbrMetallicRoughness.baseColorFactor` always exists;
`normalTexture` only if set); numeric segments index arrays
(`...baseColorFactor.3`). A `sampler2D` path must give a texture index and is
stored as the image's URI (percent-decoded), else its name if embedded; an
unnamed embedded image is an error. Missing paths warn and become `NULL`
uniforms. The shader name is the material's name (empty, with a warning, if
unnamed).

## MDX2 format

Little-endian. Every chunk, the master included, is
`FourCC | u32 payload size (unpadded) | payload | zero padding to 4 bytes`, so
chunks are always 4-byte aligned. References are absolute file offsets of the
target chunk's header. The file is a master `MDX2` chunk containing, in order:

| chunk | payload |
|---|---|
| `STRS` | NUL-terminated UTF-8 strings, deduplicated; all string fields are offsets into this |
| `ADEF` | `u32 count`, then per attribute 5×`u32`: name, components, `GL_FLOAT`, `GL_FALSE`, vertex offset |
| `VBUF` ×batch | `u32 vertexCount`, `u32` ADEF offset, `u32 stride`, then `vertexCount*stride` bytes |
| `IBUF` ×batch | `u32 indexCount`, `GL_TRIANGLES`, `GL_UNSIGNED_SHORT`, then `u16` indices |
| `MATL` ×batch | `u32` shader name, `u32` uniform count, then per uniform: name, GL type (`0` = null), size in words, value words |
| `GEOM` ×batch | `u32` offsets of its `VBUF`, `IBUF`, `MATL` |

Numeric uniform values are raw `f32`; a `sampler2D` value is a string offset
(top 4 bits reserved flags, currently 0).

## Diagnostics

Every error and warning has a stable code, `error[E0704]: ...` /
`warning[W0702]: ...`; the number is unique across both. Full list in
[`DIAGNOSTICS.md`](DIAGNOSTICS.md) (a test keeps it in sync with the source).

## Building

```sh
cargo build --release
cargo test
```

Rust 2024. `gltf` is used without its `import` feature (it would pull in image
decoders; xilit never reads pixels), so buffer loading (GLB chunk, external
files, base64 `data:` URIs) is in `gltf_util.rs`.

## Source layout

```
src/
  main.rs         entry point, mode dispatch        cli.rs       argv (clap)
  shell.rs        tokenize + dispatch                objref.rs    quoting, "Name"/#N refs
  diag.rs         diagnostic code registry, macros   vfmt.rs      --vfmt parser, layout
  gltf_util.rs    loading, accessor readers          selector.rs  selectors -> node set
  scene.rs        node hierarchy, world matrices     xform.rs     matrix math, vertex xforms
  matmap.rs       --matmap, MATL payloads            mdx2.rs      MDX2 writer
  commands/       list.rs, mdx.rs (export), mesh.rs
```