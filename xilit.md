# xilit — Initial Specification

## 1. Purpose

**xilit** is a Rust command-line tool for inspecting basic glTF 2.0 models, primarily `.glb` files, and exporting individual mesh primitives into the **MDX1** binary format.

The initial version is intentionally focused on:

* basic glTF/GLB inspection;
* exporting one mesh primitive at a time;
* producing MDX1 files with a configurable vertex layout.

The tool should not attempt to support the entire glTF ecosystem. Features/extensions that prevent an operation from being performed should result in an error.

The intended primary use case is exporting models from Blender.

---

## 2. Invocation modes

### Interactive

```text
xilit model.glb
```

opens an interactive command shell.

Errors are displayed and the shell remains active so the user can correct and repeat commands.

### Single command

```text
xilit model.glb -c 'mesh list'
```

executes the specified command(s) and exits.

Multiple commands may be supplied in the `-c` string using newlines or `;`.

### Script

```text
xilit model.glb -s script.xlt
```

executes commands from a script.

In non-interactive modes (`-c` and `-s`), an error immediately terminates execution with a non-zero exit status.

`;` separates commands generally, except when it occurs inside a JSON string.

The precise syntax/behavior of the shell, scripts, diagnostics, etc. can be implemented sensibly and refined later.

---

# 3. glTF exploration

Objects are referred to consistently throughout xilit.

A named object can be referenced by:

```text
"Object Name"
```

or directly by its zero-based glTF array index:

```text
#3
```

Names containing spaces or `/` must be JSON quoted.

Lists should display the zero-based index.

Initial commands include:

```text
list

mesh list
image list
material list
texture list
node list
scene list
```

The exact amount of information printed by the individual list commands is intentionally left open for implementation.

`list` gives a compact overview of available object types and counts, e.g.:

```text
meshes: 3
images: 2
materials: 4
textures: 2
nodes: 7
scenes: 1
```

Other basic glTF object types can be exposed similarly as useful.

No export other than mesh primitive → MDX1 is required initially.

---

# 4. glTF support

The initial implementation targets **basic glTF 2.0**, particularly `.glb`.

Extensions or other features that prevent an operation from being performed should cause an error rather than silently producing an incorrect result.

No attempt should initially be made to support complicated mesh-processing features such as Draco decoding, meshopt processing, etc., unless needed by the chosen implementation.

---

# 5. Mesh export

Syntax:

```text
mesh export <mesh>/<primitiveIndex> <filename.mdx> [--vfmt <vertexFormat>]
```

The mesh may be specified by name or zero-based index:

```text
mesh export #0/0 cube.mdx
mesh export "My Mesh"/0 cube.mdx
```

MDX1 contains **exactly one primitive**.

The mesh itself may contain multiple primitives; the primitive index selects exactly one.

---

## 6. Primitive requirements

MDX1 currently supports only:

```text
TRIANGLES
```

A primitive using another glTF primitive mode is an error.

The MDX1 header nevertheless contains a primitive-type field so future versions can support additional modes.

The primitive must have an index accessor.

A non-indexed primitive is an error.

---

## 7. Index handling

The following glTF index component types are accepted:

* `UNSIGNED_BYTE`
* `UNSIGNED_SHORT`
* `UNSIGNED_INT`

All are converted to MDX1's:

```text
u16
```

If an index cannot fit into `u16`, export fails.

MDX1's index-buffer-type header field is therefore currently always:

```text
GL_UNSIGNED_SHORT
```

The resulting indices are little-endian.

---

# 8. Vertex data

Vertex data is exported according to the requested vertex format.

All source vertex attributes used for MDX1 export must contain floating-point data.

Non-floating-point source attributes are an error.

No conversion, normalization, or integer-to-float conversion is performed.

All requested source attributes must have the same vertex count as `POSITION`. A mismatch is an error.

The vertex data is otherwise exported as-is. xilit does not compact, deduplicate, or remap vertices.

---

# 9. Default vertex format

If `--vfmt` is omitted, the default is equivalent to:

```text
position,normal,uv0
```

The default is still subject to normal validation. For example, a primitive without `NORMAL` causes export to fail rather than silently omitting it.

---

# 10. Vertex format language

The vertex format is a comma-separated list of:

```text
<attribute-name>/<binding>
```

The `/binding` portion may be omitted when the binding is the same as the attribute name.

For example:

```text
position,normal,uv0
```

is equivalent to:

```text
position/position,normal/normal,uv0/uv0
```

Attribute names are case-sensitive.

Example:

```text
position,normal,uv0
```

or:

```text
a_position/position,a_normal/normal,a_texcoord/uv0
```

The attribute names are intended to correspond to vertex shader inputs.

xilit should warn if an emitted attribute name does not look like a valid GLSL variable identifier. The warning is non-fatal and does not cause a non-zero exit status.

---

## 11. Source bindings

The initial source vocabulary is:

```text
position
normal
tangent
uv0
uv1
uv2
...
```

They map to glTF attributes:

| xilit binding | glTF attribute | components |
| ------------- | -------------- | ---------: |
| `position`    | `POSITION`     |          3 |
| `normal`      | `NORMAL`       |          3 |
| `tangent`     | `TANGENT`      |          4 |
| `uv0`         | `TEXCOORD_0`   |          2 |
| `uv1`         | `TEXCOORD_1`   |          2 |
| `uv2`         | `TEXCOORD_2`   |          2 |
| ...           | ...            |          2 |

Source names are case-sensitive.

A requested source that does not exist in the primitive is an error.

A source may only be used once in a vertex format. For example:

```text
position,previousPosition/position
```

is an error.

---

# 12. Application-filled attributes

A vertex format can reserve an attribute for the application rather than importing it from glTF.

The supported types are:

```text
float
vec2
vec3
vec4
```

For example:

```text
position/position,appCalculatedAttribute/vec4,normal,uv/uv0
```

The resulting attribute:

```text
appCalculatedAttribute/vec4
```

is allocated for every vertex and initialized to zero.

Its size is:

| type    |     size |
| ------- | -------: |
| `float` |  4 bytes |
| `vec2`  |  8 bytes |
| `vec3`  | 12 bytes |
| `vec4`  | 16 bytes |

Once written to MDX1, these attributes are indistinguishable from attributes imported from glTF.

They receive normal MDX1 attribute definitions.

---

# 13. Hidden layout entries

The special attribute name:

```text
_
```

means that the data occupies space in the vertex but **does not generate an MDX1 attribute definition**.

For example:

```text
_/position
```

copies `POSITION` into the vertex buffer but does not expose it as an MDX1 vertex attribute.

Likewise:

```text
_/vec4
```

would be an unnamed application-filled zero-initialized area if such a binding is accepted by the parser's type rules.

More importantly, padding is expressed as:

```text
_/pad4
```

which reserves exactly 4 bytes and produces no attribute definition.

`padN` accepts any positive decimal integer.

Padding contributes to both subsequent byte offsets and total vertex size.

---

# 14. Named padding / reserved attributes

A named application-filled attribute is **not** considered padding.

For example:

```text
appData/vec4
```

creates a real MDX1 attribute definition and reserves 16 zero-initialized bytes.

By contrast:

```text
_/pad16
```

is merely padding and has no MDX1 attribute definition.

---

# 15. Vertex layout

Vertex attributes and padding are packed consecutively in exactly the order specified.

There is no implicit alignment or padding.

For example:

```text
position,normal,uv0
```

produces:

```text
position    offset  0    size 12
normal      offset 12    size 12
uv0         offset 24    size  8
```

for a total vertex size of 32 bytes.

Explicit padding is the only way to alter alignment:

```text
position,normal,_/pad4,uv0
```

`padN` always adds exactly N bytes.

The MDX vertex buffer contains the resulting layout for every vertex.

---

# 16. MDX1 binary format

MDX1 is a **little-endian** binary format.

All multi-byte numeric values are little-endian.

The file starts with the literal four ASCII bytes:

```text
MDX1
```

Thus a hexadecimal dump begins:

```text
4d 44 58 31
```

and displays:

```text
MDX1
```

---

## Header

The header contains exactly **8 × u32 = 32 bytes**:

| Offset | Field                  | Type          |
| -----: | ---------------------- | ------------- |
|      0 | fourCC                 | 4 ASCII bytes |
|      4 | vertex count           | u32           |
|      8 | index count            | u32           |
|     12 | vertex attribute count | u32           |
|     16 | vertex size            | u32           |
|     20 | primitive type         | u32           |
|     24 | index buffer type      | u32           |
|     28 | string pool size       | u32           |

### fourCC

Always:

```text
MDX1
```

### Primitive type

Uses the corresponding OpenGL enum.

Currently:

```text
GL_TRIANGLES
```

is the only supported value.

### Index buffer type

Uses the OpenGL enum.

Currently:

```text
GL_UNSIGNED_SHORT
```

is the only value written.

---

# 17. String pool

Immediately after the 32-byte header comes the string pool.

It consists of a sequence of NUL-terminated strings:

```text
name1\0name2\0name3\0
```

The string pool contains **only emitted attribute names**.

Each attribute name is stored exactly once.

Hidden entries such as:

```text
_/position
_/pad4
```

do not contribute strings.

The header's `string pool size` is the exact number of bytes occupied by the entire pool, including all terminating NUL bytes padded with just enough NUL characters so the size is divisible by 4.

Attribute definition string offsets are byte offsets from the **beginning of the string pool**.

---

# 18. Vertex attribute definitions

Immediately after the string pool is the vertex attribute definition list.

There are `vertex attribute count` definitions.

Each definition consists of five little-endian `u32` fields:

| Field                 | Meaning                        |
| --------------------- | ------------------------------ |
| attribute/name offset | byte offset into string pool   |
| component count       | number of components           |
| type                  | OpenGL type enum               |
| normalized            | OpenGL boolean value           |
| vertex offset         | byte offset within each vertex |

For MDX1:

* `type` is always `GL_FLOAT`;
* `normalized` is always `GL_FALSE`;
* component count is 1, 2, 3, or 4 depending on the attribute type/source.

Examples:

```text
position → 3 × GL_FLOAT
normal   → 3 × GL_FLOAT
tangent  → 4 × GL_FLOAT
uv0      → 2 × GL_FLOAT
foo/vec4 → 4 × GL_FLOAT
```

Hidden layout entries do not appear in this list.

---

# 19. Vertex buffer

The vertex buffer immediately follows the attribute-definition list.

It contains:

```text
vertex count × vertex size
```

bytes.

The layout is exactly the layout generated from `--vfmt`.

Imported floating-point glTF attributes are written as little-endian IEEE-754 `f32` values.

Application-filled attributes are zero-initialized.

Padding bytes are zero-initialized.

---

# 20. Index buffer

The index buffer immediately follows the vertex buffer.

It contains:

```text
index count × 2
```

bytes because MDX1 currently uses `u16`.

Indices are little-endian.

---

# 21. Resulting file structure

An MDX1 file is therefore:

```text
+-------------------------------+
| MDX1 header (32 bytes)        |
+-------------------------------+
| string pool                   |
+-------------------------------+
| vertex attribute definitions  |
| (20 bytes each)               |
+-------------------------------+
| vertex buffer                 |
+-------------------------------+
| u16 index buffer              |
+-------------------------------+
```

This deliberately keeps the binary format simple while leaving room for future MDX versions.

---

## 22. Export validation summary

An MDX1 export fails if, among other things:

* the requested mesh/primitive doesn't exist;
* the primitive isn't `TRIANGLES`;
* the primitive has no indices;
* an index cannot fit in `u16`;
* a requested glTF source attribute doesn't exist;
* a source attribute isn't floating-point;
* source attribute counts don't match `POSITION`;
* a source binding is specified more than once;
* the vertex format contains an invalid type/binding combination;
* the underlying glTF data uses an unsupported feature that prevents correct export.

Warnings, such as a suspicious GLSL attribute name, do **not** make an otherwise successful export fail.
