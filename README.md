# 3mf-dll

Reads 3MF packages (3D Manufacturing Format) into placed, coloured triangle meshes, for
[Open BIM Suite](https://github.com/OpenAEC-Foundation/open-bim-suite) and its viewer Open 3D
Viewer Studio. Pure Rust on the bytes of the file, so it also runs in the browser (WebAssembly).

## Crates

| Crate | What it holds |
|---|---|
| `threemf-model` | The plain model the reader returns: one `Item` per build item, its triangles in metres, Z up, with its colours and properties. |
| `threemf-read` | The reader: the package, its model parts, objects, components and build. |

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
