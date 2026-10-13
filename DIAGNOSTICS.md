# Diagnostics

Every error and warning xilit prints carries a code: `error[E0302]: ...` / `warning[W0508]: ...`.

The first letter is the severity (`E` error, `W` warning); the 4-digit number is unique across both,
so a code never changes meaning if its severity changes. Errors abort `-c`/`-s` runs (exit status 1);
warnings never affect the exit status. Command-line usage errors from `clap` itself (e.g. a missing
model argument) are not coded.


## 00xx — General

| Code | Meaning |
|---|---|
| `E0001` | Unclassified failure (an internal error path is missing a code) |

## 01xx — Command line / shell

| Code | Meaning |
|---|---|
| `E0101` | `-c` and `-s` were both given |
| `E0102` | Unknown command |
| `E0103` | Wrong or missing subcommand/arguments for a command |
| `E0104` | Unexpected extra argument |
| `E0105` | An option that needs a value was given none |
| `E0106` | Could not read the script file |
| `E0107` | Could not read from stdin in the interactive shell |

## 02xx — Quoting and object references

| Code | Meaning |
|---|---|
| `E0201` | Unterminated quoted string |
| `E0202` | Invalid escape sequence in a quoted string |
| `E0203` | Malformed object reference (expected `\"Name\"` or `#index`) |
| `E0204` | Malformed `<mesh>/<primitiveIndex>` reference |

## 03xx — Model loading

| Code | Meaning |
|---|---|
| `E0301` | Could not read the model file |
| `E0302` | The glTF failed validation (`--lenient` may load it anyway) |
| `E0303` | The glTF could not be parsed even without validation |
| `E0304` | A buffer refers to the GLB binary chunk, but there is none |
| `E0305` | Malformed `data:` URI in a buffer |
| `E0306` | Unsupported `data:` URI (only base64 payloads are supported) |
| `E0307` | Could not base64-decode a `data:` URI |
| `E0308` | Could not read an external buffer file |
| `E0309` | A buffer holds less data than it declares |
| `W0310` | glTF validation problems were ignored because of `--lenient` |

## 04xx — Accessor reading

| Code | Meaning |
|---|---|
| `E0401` | Sparse accessors are not supported |
| `E0402` | Accessor has no buffer view |
| `E0403` | Accessor refers to a nonexistent buffer |
| `E0404` | Accessor extends past the end of its buffer |
| `E0405` | A source attribute is not floating-point (no conversion is done) |
| `E0406` | Index accessor is not UNSIGNED_BYTE/SHORT/INT |

## 05xx — Vertex format

| Code | Meaning |
|---|---|
| `E0501` | Empty field in the vertex format |
| `E0502` | Empty attribute name in the vertex format |
| `E0503` | Unknown binding in the vertex format |
| `E0504` | Padding amount is zero or too large |
| `E0505` | `padN` must use the hidden name `_` |
| `E0506` | A source binding is used more than once |
| `E0507` | The vertex layout is larger than 4 GiB |
| `W0508` | An emitted attribute name is not a valid GLSL identifier |

## 06xx — Mesh export

| Code | Meaning |
|---|---|
| `E0601` | No such mesh |
| `E0602` | The mesh has no such primitive |
| `E0603` | Primitive mode is not TRIANGLES |
| `E0604` | Primitive has no index accessor |
| `E0605` | An index does not fit into u16 |
| `E0606` | Primitive has no POSITION attribute |
| `E0607` | A requested source attribute does not exist in the primitive |
| `E0608` | A source attribute's vertex count differs from POSITION |
| `E0609` | A source attribute has an unexpected component count |
| `E0610` | Could not create the output file |
| `E0611` | Could not write the output file |
