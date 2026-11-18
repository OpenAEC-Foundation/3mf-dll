//! 3MF (3D Manufacturing Format): a ZIP package with XML model parts.
//!
//! Each build item is an [`Item`](Mesh) with its name; components are placed with their
//! transforms, also from other model parts (production extension). From the materials
//! extension: base materials, colour groups, composite materials, multi-properties and textures
//! (sampled at each corner) colour the triangles, corner by corner. The beam lattice extension's
//! beams and balls are meshed as cones, spheres and caps; the slice extension's slices are drawn
//! as their contour lines at their height. The project files of slicers are read for what they
//! say of the model: the plate each item stands on, the names of objects and parts, and the
//! filament colour each part prints in; their print settings are left alone. A sliced file that
//! holds only G-code says so.
//!
//! Z up; converted to metres.

use glam::{Mat4, Vec3, Vec4};
use std::collections::HashMap;
use std::io::Read;
pub use threemf_model::{Item as Mesh, DEFAULT_COLOR};

mod slicer;
use slicer::Slicer;

/// Reads a 3MF package: one mesh per build item.
pub fn read(data: &[u8]) -> Result<Vec<Mesh>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("3MF: {e}"))?;
    let root = root_part(&mut archive).unwrap_or_else(|| "3D/3dmodel.model".into());
    let entries: Vec<String> = (0..archive.len()).filter_map(|i| archive.name_for_index(i)?.ok().map(|n| n.into_owned())).collect();
    // every model part: the root and the parts its components and slices may point to
    let names: Vec<&String> = entries.iter().filter(|n| n.to_lowercase().ends_with(".model")).collect();
    let mut package = Package::default();
    let mut build = None;
    let mut build_part = String::new();
    for name in names {
        let text = read_entry(&mut archive, name)?;
        let part = parse_part(name, &text, &mut package)?;
        if name.eq_ignore_ascii_case(&root) {
            build = Some(part);
            build_part = name.clone();
        }
    }
    let gcode: Vec<&String> = entries.iter().filter(|n| n.to_lowercase().ends_with(".gcode")).collect();
    let Some((scale, items)) = build else {
        if !gcode.is_empty() {
            return Err(sliced_only(&gcode));
        }
        return Err(format!("3MF: model part {root} not found"));
    };
    let mut slicer = Slicer::read(&mut archive, &entries);
    slicer.program = package.applications.get(&build_part).cloned();
    let mut images = Images::default();
    let mut meshes = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let mut mesh = Triangles::default();
        let parts = slicer.parts.get(&item.object);
        let fallback = slicer.objects.get(&item.object).and_then(|o| o.color);
        let world = Mat4::from_scale(Vec3::splat(scale)) * item.transform;
        mesh.add(&package, &mut images, &mut archive, (&item.path, item.object), world, 0, fallback, parts, &slicer);
        if mesh.positions.is_empty() && mesh.lines.is_empty() {
            continue;
        }
        let object = package.objects.get(&(item.path.clone(), item.object));
        let named = slicer.objects.get(&item.object).and_then(|o| o.name.clone());
        let name = named
            .or_else(|| object.and_then(|o| o.name.clone().or_else(|| o.part_number.clone())))
            .unwrap_or_else(|| format!("Item {}", n + 1));
        let mut props = vec![("Name".to_string(), name.clone()), ("Object id".to_string(), item.object.to_string())];
        if let Some(number) = item.part_number.as_ref().or(object.and_then(|o| o.part_number.as_ref())) {
            props.push(("Part number".into(), number.clone()));
        }
        if let Some(plate) = slicer.plate_of(item.object) {
            props.push(("Plate".into(), plate));
        }
        if let Some(program) = &slicer.program {
            props.push(("Made with".into(), program.clone()));
        }
        if mesh.beams > 0 {
            props.push(("Beams".into(), mesh.beams.to_string()));
        }
        if mesh.slices > 0 {
            props.push(("Slices".into(), mesh.slices.to_string()));
        }
        // one colour for the whole item: the item colour, no vertex colours
        let all: Vec<[u8; 4]> = mesh.colors.iter().chain(&mesh.line_colors).copied().collect();
        let uniform = all.windows(2).all(|w| w[0] == w[1]);
        let color = all.first().map(|c| c.map(|v| v as f32 / 255.0)).map(|c| [c[0], c[1], c[2]]).unwrap_or(DEFAULT_COLOR);
        let (colors, line_colors) = if uniform { (None, None) } else { (Some(mesh.colors), Some(mesh.line_colors)) };
        meshes.push(Mesh { name, color, positions: mesh.positions, normals: mesh.normals, colors, lines: mesh.lines, line_colors, props });
    }
    if meshes.is_empty() && !gcode.is_empty() {
        return Err(sliced_only(&gcode));
    }
    Ok(meshes)
}

/// The message for a sliced file without a model: its plates' G-code only.
fn sliced_only(gcode: &[&String]) -> String {
    format!("3MF: this sliced file holds only the G-code of {} plate(s) and no model to show", gcode.len())
}

/// The 3D model part named in `_rels/.rels`.
fn root_part(archive: &mut Archive) -> Option<String> {
    let text = read_entry(archive, "_rels/.rels").ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let target = doc
        .descendants()
        .filter(|n| n.has_tag_name("Relationship"))
        .find(|n| n.attribute("Type").is_some_and(|t| t.ends_with("/3dmodel")))?
        .attribute("Target")?;
    Some(target.trim_start_matches('/').to_string())
}

type Archive<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;

fn read_bytes(archive: &mut Archive, name: &str) -> Result<Vec<u8>, String> {
    let name = name.trim_start_matches('/');
    let index = archive.index_for_name(name).or_else(|| {
        // part names are case-insensitive in the package
        (0..archive.len()).find(|&i| archive.name_for_index(i).and_then(|n| n.ok()).is_some_and(|n| n.eq_ignore_ascii_case(name)))
    });
    let mut file = index.and_then(|i| archive.by_index(i).ok()).ok_or_else(|| format!("3MF {name}: not in the package"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| format!("3MF {name}: {e}"))?;
    Ok(bytes)
}

fn read_entry(archive: &mut Archive, name: &str) -> Result<String, String> {
    let bytes = read_bytes(archive, name)?;
    String::from_utf8(bytes).map_err(|e| format!("3MF {name}: {e}"))
}

/// What the model parts hold, keyed by part path and resource id.
#[derive(Default)]
struct Package {
    objects: HashMap<(String, u32), Object>,
    groups: HashMap<(String, u32), Group>,
    textures: HashMap<(String, u32), Texture>,
    slices: HashMap<(String, u32), SliceStack>,
    /// The program each part says wrote it (`<metadata name="Application">`).
    applications: HashMap<String, String>,
}

/// A property group: what an index into it gives a corner.
enum Group {
    /// Base materials, colour groups and composite materials (mixed once).
    Colors(Vec<[u8; 4]>),
    /// Texture coordinates into a texture.
    Texture { texture: u32, coords: Vec<[f32; 2]> },
    /// Multi-properties: per index one index into each of the groups `pids`, layered.
    Multi { pids: Vec<u32>, blends: Vec<bool>, multis: Vec<Vec<usize>> },
}

/// A texture: its image part and how coordinates outside 0..1 are taken.
struct Texture {
    path: String,
    clamp: [bool; 2],
}

/// An object of a model part: a mesh, or components that place other objects, with what the
/// extensions add to it.
struct Object {
    name: Option<String>,
    part_number: Option<String>,
    vertices: Vec<Vec3>,
    /// Corner indices and the property of each corner (group, index).
    triangles: Vec<([u32; 3], [Option<(u32, usize)>; 3])>,
    components: Vec<Item>,
    beams: Option<Lattice>,
    /// The slice stack the object is sliced into.
    slices: Option<u32>,
}

/// A build item or a component: an object (in a part) and where it goes.
struct Item {
    path: String,
    object: u32,
    transform: Mat4,
    part_number: Option<String>,
}

/// A beam lattice: beams between vertices with a radius at each end, and balls on vertices.
struct Lattice {
    beams: Vec<Beam>,
    balls: Vec<(u32, f32, Option<(u32, usize)>)>,
}

#[derive(Clone, Copy, PartialEq)]
enum Cap {
    Sphere,
    Hemisphere,
    Butt,
}

struct Beam {
    v: [u32; 2],
    r: [f32; 2],
    cap: [Cap; 2],
    property: [Option<(u32, usize)>; 2],
}

/// A slice stack: its slices (or references to stacks in other parts).
struct SliceStack {
    slices: Vec<Slice>,
    refs: Vec<(String, u32)>,
}

/// One slice: its height and its polygons as closed or open polylines.
struct Slice {
    z: f32,
    polygons: Vec<Vec<[f32; 2]>>,
}

fn attr<T: std::str::FromStr>(node: roxmltree::Node, name: &str) -> Option<T> {
    node.attributes().find(|a| a.name() == name).and_then(|a| a.value().trim().parse().ok())
}

/// Reads the resources of one model part into `package`; returns its unit (metres) and build items.
fn parse_part(path: &str, text: &str, package: &mut Package) -> Result<(f32, Vec<Item>), String> {
    let doc = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|e| format!("3MF {path}: {e}"))?;
    let model = doc.root_element();
    let scale = match model.attribute("unit").unwrap_or("millimeter") {
        "micron" => 1e-6,
        "centimeter" => 0.01,
        "inch" => 0.0254,
        "foot" => 0.3048,
        "meter" => 1.0,
        _ => 0.001,
    };
    if let Some(app) = model.children().find(|n| n.has_tag_name("metadata") && n.attribute("name") == Some("Application")).and_then(|n| n.text()) {
        package.applications.insert(path.to_string(), app.trim().to_string());
    }
    let key = |id: u32| (path.to_string(), id);
    // property groups, by id (namespaces are not compared); composites mix base materials
    // read before them, so the groups go in document order
    for group in model.descendants().filter(|n| n.is_element()) {
        let Some(id) = attr::<u32>(group, "id") else { continue };
        match group.tag_name().name() {
            "basematerials" | "colorgroup" => {
                let colors = elements(group)
                    .into_iter()
                    .map(|n| n.attribute("displaycolor").or(n.attribute("color")).and_then(parse_color).unwrap_or([184, 184, 189, 255]))
                    .collect();
                package.groups.insert(key(id), Group::Colors(colors));
            }
            "compositematerials" => {
                let base = attr::<u32>(group, "matid").and_then(|m| match package.groups.get(&key(m)) {
                    Some(Group::Colors(c)) => Some(c.clone()),
                    _ => None,
                });
                let indices: Vec<usize> = group.attribute("matindices").unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect();
                let colors = elements(group)
                    .into_iter()
                    .map(|n| {
                        let values: Vec<f32> = n.attribute("values").unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect();
                        let total: f32 = values.iter().sum::<f32>().max(1e-6);
                        let mut mix = [0.0f32; 4];
                        for (w, i) in values.iter().zip(&indices) {
                            let c = base.as_ref().and_then(|b| b.get(*i)).copied().unwrap_or([184, 184, 189, 255]);
                            for k in 0..4 {
                                mix[k] += c[k] as f32 * w / total;
                            }
                        }
                        mix.map(|v| v.round().clamp(0.0, 255.0) as u8)
                    })
                    .collect();
                package.groups.insert(key(id), Group::Colors(colors));
            }
            "texture2d" => {
                let Some(texture) = group.attribute("path") else { continue };
                let style = |a: &str| matches!(group.attribute(a), Some("clamp") | Some("none"));
                package.textures.insert(key(id), Texture { path: texture.trim_start_matches('/').to_string(), clamp: [style("tilestyleu"), style("tilestylev")] });
            }
            "texture2dgroup" => {
                let Some(texture) = attr::<u32>(group, "texid") else { continue };
                let coords = elements(group).into_iter().map(|n| [attr::<f32>(n, "u").unwrap_or(0.0), attr::<f32>(n, "v").unwrap_or(0.0)]).collect();
                package.groups.insert(key(id), Group::Texture { texture, coords });
            }
            "multiproperties" => {
                let pids: Vec<u32> = group.attribute("pids").unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect();
                let blends: Vec<bool> = group.attribute("blendmethods").unwrap_or("").split_whitespace().map(|b| b == "multiply").collect();
                let multis = elements(group).into_iter().map(|n| n.attribute("pindices").unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect()).collect();
                package.groups.insert(key(id), Group::Multi { pids, blends, multis });
            }
            "slicestack" => {
                let mut stack = SliceStack { slices: Vec::new(), refs: Vec::new() };
                for n in elements(group) {
                    match n.tag_name().name() {
                        "slice" => stack.slices.push(parse_slice(n)),
                        "sliceref" => {
                            let Some(stack_id) = attr::<u32>(n, "slicestackid") else { continue };
                            let part = n.attribute("slicepath").map(|p| p.trim_start_matches('/').to_string()).unwrap_or_else(|| path.to_string());
                            stack.refs.push((part, stack_id));
                        }
                        _ => {}
                    }
                }
                package.slices.insert(key(id), stack);
            }
            _ => {}
        }
    }
    for node in model.descendants().filter(|n| n.has_tag_name("object")) {
        let Some(id) = attr::<u32>(node, "id") else { continue };
        let pid = attr::<u32>(node, "pid");
        let pindex = attr::<usize>(node, "pindex");
        let mut object = Object {
            name: node.attribute("name").filter(|s| !s.is_empty()).map(String::from),
            part_number: node.attribute("partnumber").map(String::from),
            vertices: Vec::new(),
            triangles: Vec::new(),
            components: Vec::new(),
            beams: None,
            slices: attr::<u32>(node, "slicestackid"),
        };
        for n in node.descendants().filter(|n| n.is_element()) {
            match n.tag_name().name() {
                "vertex" => object.vertices.push(Vec3::new(num(n, "x"), num(n, "y"), num(n, "z"))),
                "triangle" => {
                    let v = ["v1", "v2", "v3"].map(|a| attr::<u32>(n, a).unwrap_or(u32::MAX));
                    let tri_pid = attr::<u32>(n, "pid").or(pid);
                    let p1 = attr::<usize>(n, "p1").or(pindex);
                    let corners = match (tri_pid, p1) {
                        (Some(group), Some(p1)) => [Some((group, p1)), Some((group, attr(n, "p2").unwrap_or(p1))), Some((group, attr(n, "p3").unwrap_or(p1)))],
                        (Some(group), None) => [Some((group, 0)); 3],
                        _ => [None; 3],
                    };
                    object.triangles.push((v, corners));
                }
                "component" => {
                    let Some(object_id) = attr::<u32>(n, "objectid") else { continue };
                    // production extension: p:path to another model part
                    let part = n.attributes().find(|a| a.name() == "path").map(|a| a.value().trim_start_matches('/').to_string());
                    object.components.push(Item {
                        path: part.unwrap_or_else(|| path.to_string()),
                        object: object_id,
                        transform: transform(n.attribute("transform")),
                        part_number: None,
                    });
                }
                "beamlattice" => object.beams = Some(parse_lattice(n, pid, pindex)),
                _ => {}
            }
        }
        package.objects.insert(key(id), object);
    }
    let items = model
        .children()
        .filter(|n| n.has_tag_name("build"))
        .flat_map(|b| b.children().filter(|n| n.has_tag_name("item")))
        .filter_map(|n| {
            Some(Item {
                path: path.to_string(),
                object: attr(n, "objectid")?,
                transform: transform(n.attribute("transform")),
                part_number: n.attribute("partnumber").map(String::from),
            })
        })
        .collect();
    Ok((scale, items))
}

/// The child elements of `n`.
fn elements<'a, 'i>(n: roxmltree::Node<'a, 'i>) -> Vec<roxmltree::Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}

fn parse_slice(node: roxmltree::Node) -> Slice {
    let mut vertices = Vec::new();
    let mut polygons = Vec::new();
    for n in node.children().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "vertices" => vertices = n.children().filter(|v| v.is_element()).map(|v| [num(v, "x"), num(v, "y")]).collect(),
            "polygon" => {
                let at = |i: usize| vertices.get(i).copied();
                let mut line: Vec<[f32; 2]> = attr::<usize>(n, "startv").and_then(at).into_iter().collect();
                line.extend(n.children().filter(|s| s.is_element()).filter_map(|s| attr::<usize>(s, "v2").and_then(at)));
                if line.len() > 1 {
                    polygons.push(line);
                }
            }
            _ => {}
        }
    }
    Slice { z: num(node, "ztop"), polygons }
}

fn parse_lattice(node: roxmltree::Node, pid: Option<u32>, pindex: Option<usize>) -> Lattice {
    let radius = attr::<f32>(node, "radius").unwrap_or(1.0);
    let cap = |v: Option<&str>| match v {
        Some("hemisphere") => Cap::Hemisphere,
        Some("butt") => Cap::Butt,
        _ => Cap::Sphere,
    };
    let default_cap = cap(node.attribute("cap"));
    let ball_radius = attr::<f32>(node, "ballradius").unwrap_or(radius);
    let mut lattice = Lattice { beams: Vec::new(), balls: Vec::new() };
    let property = |n: roxmltree::Node, a: &str| {
        let group = attr::<u32>(n, "pid").or(pid)?;
        Some((group, attr::<usize>(n, a).or(attr::<usize>(n, "p1")).or(pindex).unwrap_or(0)))
    };
    for n in node.descendants().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "beam" => {
                let (Some(v1), Some(v2)) = (attr::<u32>(n, "v1"), attr::<u32>(n, "v2")) else { continue };
                let r1 = attr::<f32>(n, "r1").unwrap_or(radius);
                let r2 = attr::<f32>(n, "r2").unwrap_or(r1);
                let caps = [cap(n.attribute("cap1").or(node.attribute("cap"))), cap(n.attribute("cap2").or(node.attribute("cap")))];
                let caps = if n.attribute("cap1").is_none() && n.attribute("cap2").is_none() { [default_cap; 2] } else { caps };
                lattice.beams.push(Beam { v: [v1, v2], r: [r1, r2], cap: caps, property: [property(n, "p1"), property(n, "p2")] });
            }
            "ball" => {
                let Some(v) = attr::<u32>(n, "vindex") else { continue };
                lattice.balls.push((v, attr::<f32>(n, "r").unwrap_or(ball_radius), property(n, "p")));
            }
            _ => {}
        }
    }
    lattice
}

fn num(node: roxmltree::Node, attribute: &str) -> f32 {
    attr(node, attribute).unwrap_or(0.0)
}

/// `m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32`: the 3MF row-vector matrix, so each triple
/// is a column of glam's column-vector matrix.
fn transform(value: Option<&str>) -> Mat4 {
    let Some(values) = value.map(|v| v.split_whitespace().filter_map(|s| s.parse::<f32>().ok()).collect::<Vec<_>>()) else {
        return Mat4::IDENTITY;
    };
    if values.len() != 12 {
        return Mat4::IDENTITY;
    }
    let column = |i: usize, w: f32| Vec4::new(values[3 * i], values[3 * i + 1], values[3 * i + 2], w);
    Mat4::from_cols(column(0, 0.0), column(1, 0.0), column(2, 0.0), column(3, 1.0))
}

/// `#RRGGBB` or `#RRGGBBAA`, sRGB.
fn parse_color(value: &str) -> Option<[u8; 4]> {
    let hex = value.trim().strip_prefix('#')?;
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    match hex.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// The decoded texture images, by part path (`None`: it could not be read).
#[derive(Default)]
struct Images {
    images: HashMap<String, Option<image::RgbaImage>>,
}

impl Images {
    fn sample(&mut self, archive: &mut Archive, texture: &Texture, uv: [f32; 2]) -> Option<[u8; 4]> {
        let image = self
            .images
            .entry(texture.path.clone())
            .or_insert_with(|| read_bytes(archive, &texture.path).ok().and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8()))
            .as_ref()?;
        let wrap = |t: f32, clamp: bool| if clamp { t.clamp(0.0, 1.0) } else { t - t.floor() };
        let (u, v) = (wrap(uv[0], texture.clamp[0]), wrap(uv[1], texture.clamp[1]));
        let (w, h) = image.dimensions();
        // v runs up from the bottom of the image
        let x = ((u * w as f32) as u32).min(w.saturating_sub(1));
        let y = (((1.0 - v) * h as f32) as u32).min(h.saturating_sub(1));
        Some(image.get_pixel(x, y).0)
    }
}

/// Triangles and lines of one build item in world space.
#[derive(Default)]
struct Triangles {
    positions: Vec<f32>,
    normals: Vec<f32>,
    colors: Vec<[u8; 4]>,
    lines: Vec<f32>,
    line_colors: Vec<[u8; 4]>,
    beams: usize,
    slices: usize,
}

const DEFAULT_RGBA: [u8; 4] = [184, 184, 189, 255];

impl Triangles {
    /// The colour a corner's property gives, in the part `path`.
    fn color(package: &Package, images: &mut Images, archive: &mut Archive, path: &str, property: (u32, usize), depth: usize) -> Option<[u8; 4]> {
        match package.groups.get(&(path.to_string(), property.0))? {
            Group::Colors(colors) => colors.get(property.1).copied(),
            Group::Texture { texture, coords } => {
                let texture = package.textures.get(&(path.to_string(), *texture))?;
                images.sample(archive, texture, *coords.get(property.1)?)
            }
            Group::Multi { pids, blends, multis } if depth < 4 => {
                let layers = multis.get(property.1)?;
                let mut out: Option<[f32; 4]> = None;
                for (k, (&group, &index)) in pids.iter().zip(layers).enumerate() {
                    let Some(c) = Self::color(package, images, archive, path, (group, index), depth + 1) else { continue };
                    let c = c.map(|v| v as f32 / 255.0);
                    out = Some(match out {
                        None => c,
                        Some(base) if blends.get(k.wrapping_sub(1)).copied().unwrap_or(false) => [0, 1, 2, 3].map(|i| base[i] * c[i]),
                        Some(base) => [0, 1, 2].map(|i| base[i] * (1.0 - c[3]) + c[i] * c[3]).into_iter().chain([base[3]]).collect::<Vec<_>>().try_into().unwrap(),
                    });
                }
                out.map(|c| c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
            }
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, package: &Package, images: &mut Images, archive: &mut Archive, key: (&String, u32), world: Mat4, depth: usize, fallback: Option<[u8; 4]>, parts: Option<&HashMap<u32, [u8; 4]>>, slicer: &Slicer) {
        // components may not refer back to themselves, but a broken file could
        let Some(object) = package.objects.get(&(key.0.clone(), key.1)).filter(|_| depth < 32) else { return };
        let mirrored = world.determinant() < 0.0;
        let ranges = slicer.ranges.get(&key.1).filter(|_| depth == 0);
        let base = fallback.unwrap_or(DEFAULT_RGBA);
        for (t, &(v, props)) in object.triangles.iter().enumerate() {
            let Some(mut p) = v.iter().map(|&i| object.vertices.get(i as usize).map(|&p| world.transform_point3(p))).collect::<Option<Vec<_>>>() else {
                continue;
            };
            // a part painted in the slicer's filament colour, when the file gives the triangle none
            let range = ranges.and_then(|r| r.iter().find(|(first, last, _)| (*first..=*last).contains(&(t as u32)))).map(|r| r.2);
            let mut rgba = props.map(|prop| prop.and_then(|prop| Self::color(package, images, archive, key.0, prop, 0)).unwrap_or(range.unwrap_or(base)));
            if mirrored {
                p.swap(1, 2);
                rgba.swap(1, 2);
            }
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            for (corner, c) in p.into_iter().zip(rgba) {
                self.positions.extend_from_slice(&corner.to_array());
                self.normals.extend_from_slice(&n.to_array());
                self.colors.push(c);
            }
        }
        if let Some(lattice) = &object.beams {
            self.lattice(package, images, archive, key.0, object, lattice, world, base);
        }
        if let Some(stack) = object.slices {
            let color = base;
            self.slice_lines(package, (key.0, stack), world, color, 0);
        }
        for c in &object.components {
            let fallback = parts.and_then(|p| p.get(&c.object)).copied().or(fallback);
            self.add(package, images, archive, (&c.path, c.object), world * c.transform, depth + 1, fallback, None, slicer);
        }
    }

    fn slice_lines(&mut self, package: &Package, key: (&String, u32), world: Mat4, color: [u8; 4], depth: usize) {
        let Some(stack) = package.slices.get(&(key.0.clone(), key.1)).filter(|_| depth < 8) else { return };
        for slice in &stack.slices {
            self.slices += 1;
            for polygon in &slice.polygons {
                for pair in polygon.windows(2) {
                    for q in pair {
                        let p = world.transform_point3(Vec3::new(q[0], q[1], slice.z));
                        self.lines.extend_from_slice(&p.to_array());
                        self.line_colors.push(color);
                    }
                }
            }
        }
        for (path, id) in &stack.refs {
            self.slice_lines(package, (path, *id), world, color, depth + 1);
        }
    }

    /// Meshes the beams (cones between their ends' radii, with their caps) and balls.
    #[allow(clippy::too_many_arguments)]
    fn lattice(&mut self, package: &Package, images: &mut Images, archive: &mut Archive, path: &str, object: &Object, lattice: &Lattice, world: Mat4, base: [u8; 4]) {
        // fewer sides for a large lattice, so it stays light
        let sides = if lattice.beams.len() > 20_000 { 6 } else { 12 };
        let mut color = |prop: Option<(u32, usize)>| prop.and_then(|p| Self::color(package, images, archive, path, p, 0)).unwrap_or(base);
        for beam in &lattice.beams {
            let (Some(&a), Some(&b)) = (object.vertices.get(beam.v[0] as usize), object.vertices.get(beam.v[1] as usize)) else { continue };
            self.beams += 1;
            let colors = [color(beam.property[0]), color(beam.property[1])];
            let axis = b - a;
            if axis.length_squared() < 1e-12 {
                continue;
            }
            let dir = axis.normalize();
            let (u, w) = dir.any_orthonormal_pair();
            let ring = |centre: Vec3, r: f32, k: usize| {
                let t = k as f32 / sides as f32 * std::f32::consts::TAU;
                let n = u * t.cos() + w * t.sin();
                (centre + n * r, n)
            };
            for k in 0..sides {
                let (p0, n0) = ring(a, beam.r[0], k);
                let (p1, n1) = ring(a, beam.r[0], k + 1);
                let (q0, m0) = ring(b, beam.r[1], k);
                let (q1, m1) = ring(b, beam.r[1], k + 1);
                self.triangle(world, [p0, p1, q1], [n0, n1, m1], [colors[0], colors[0], colors[1]]);
                self.triangle(world, [p0, q1, q0], [n0, m1, m0], [colors[0], colors[1], colors[1]]);
            }
            for (end, centre, r, outward) in [(0usize, a, beam.r[0], -dir), (1, b, beam.r[1], dir)] {
                match beam.cap[end] {
                    Cap::Butt => {
                        for k in 0..sides {
                            let (p0, _) = ring(centre, r, k);
                            let (p1, _) = ring(centre, r, k + 1);
                            let tri = if end == 0 { [centre, p1, p0] } else { [centre, p0, p1] };
                            self.triangle(world, tri, [outward; 3], [colors[end]; 3]);
                        }
                    }
                    Cap::Sphere => self.ball(world, centre, r, colors[end], None, sides),
                    Cap::Hemisphere => self.ball(world, centre, r, colors[end], Some(outward), sides),
                }
            }
        }
        for &(v, r, prop) in &lattice.balls {
            let Some(&centre) = object.vertices.get(v as usize) else { continue };
            let c = color(prop);
            self.ball(world, centre, r, c, None, sides);
        }
    }

    /// A sphere, or with `half` the half sphere on that side.
    fn ball(&mut self, world: Mat4, centre: Vec3, r: f32, color: [u8; 4], half: Option<Vec3>, sides: usize) {
        let axis = half.unwrap_or(Vec3::Z);
        let (u, w) = axis.any_orthonormal_pair();
        let rows = sides / 2;
        let start = if half.is_some() { rows / 2 } else { 0 };
        let at = |i: usize, k: usize| {
            let theta = i as f32 / rows as f32 * std::f32::consts::PI;
            let phi = k as f32 / sides as f32 * std::f32::consts::TAU;
            let n = -axis * theta.cos() + (u * phi.cos() + w * phi.sin()) * theta.sin();
            (centre + n * r, n)
        };
        for i in start..rows {
            for k in 0..sides {
                let (a, na) = at(i, k);
                let (b, nb) = at(i, k + 1);
                let (c, nc) = at(i + 1, k + 1);
                let (d, nd) = at(i + 1, k);
                if i > 0 {
                    self.triangle(world, [a, b, c], [na, nb, nc], [color; 3]);
                }
                if i + 1 < rows {
                    self.triangle(world, [a, c, d], [na, nc, nd], [color; 3]);
                }
            }
        }
    }

    /// One triangle given in object coordinates, outward by its normals.
    fn triangle(&mut self, world: Mat4, p: [Vec3; 3], n: [Vec3; 3], c: [[u8; 4]; 3]) {
        let mut p = p.map(|p| world.transform_point3(p));
        let normal = world.inverse().transpose();
        let mut n = n.map(|n| normal.transform_vector3(n).normalize_or_zero());
        let mut c = c;
        let face = (p[1] - p[0]).cross(p[2] - p[0]);
        if face.dot(n[0] + n[1] + n[2]) < 0.0 {
            p.swap(1, 2);
            n.swap(1, 2);
            c.swap(1, 2);
        }
        for k in 0..3 {
            self.positions.extend_from_slice(&p[k].to_array());
            self.normals.extend_from_slice(&n[k].to_array());
            self.colors.push(c[k]);
        }
    }
}

#[cfg(test)]
mod tests;
