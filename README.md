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