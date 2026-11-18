# 3mf-dll

Reads 3MF packages (3D Manufacturing Format) into placed, coloured triangle meshes, for
[Open BIM Suite](https://github.com/OpenAEC-Foundation/open-bim-suite) and its viewer Open 3D
Viewer Studio. Pure Rust on the bytes of the file, so it also runs in the browser (WebAssembly).

## What it reads

- **Core**: the build items, their objects and components with their transforms, in the unit of
  the file, returned in metres, Z up.
- **Production extension**: components that place objects of other model parts of the package.
- **Materials extension**: base materials, colour groups, composite materials (mixed), multi-
  properties (layered, mixed or multiplied) and textures (sampled at each corner of a triangle),
  corner by corner.
- **Beam lattice extension**: beams as cones between their two radii with sphere, half-sphere or
  flat caps, and balls.
- **Slice extension**: the slices of a slice stack (also in other parts) as their contour lines
  at their height.
- **Slicer project files**: the names of objects and parts, the filament each prints with and
  that filament's colour, and the plate each object stands on, from
  `Metadata/model_settings.config` with `Metadata/project_settings.config`, or from
  `Metadata/Slic3r_PE_model.config` with `Metadata/Slic3r_PE.config`. Print settings are left
  alone. A sliced package that holds only G-code says so.

Not read: colours painted on single triangles in a slicer, displacement and volumetric
extensions, and the textures themselves (only their colours at the corners).

## Crates

| Crate | What it holds |
|---|---|
| `threemf-model` | The plain model the reader returns: one `Item` per build item, its triangles (and slice contours) in metres, Z up, with its colours and properties. |
| `threemf-read` | The reader: the package, its model parts, objects, components, extensions and build, and the slicer project files. |

## Use

```rust
let items = threemf_read::read(&std::fs::read("part.3mf")?)?;
for item in items {
    println!("{}: {} triangles", item.name, item.positions.len() / 9);
}
```

## Test data

`crates/threemf-read/tests/data` holds sample files of the 3MF Consortium
([3mf-samples](https://github.com/3MFConsortium/3mf-samples), BSD 2-Clause, see
`LICENSE-3mf-samples` there).

## Licence

LGPL-3.0-or-later.
