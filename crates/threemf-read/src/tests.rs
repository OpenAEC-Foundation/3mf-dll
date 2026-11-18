use super::*;
use std::io::Write;

/// A package with `files` beside its root model part `3D/main.model`.
fn package(model: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("_rels/.rels", options).unwrap();
    zip.write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/main.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#).unwrap();
    zip.start_file("3D/main.model", options).unwrap();
    zip.write_all(model.as_bytes()).unwrap();
    for (name, data) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}

fn sample(name: &str) -> Vec<Mesh> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    read(&std::fs::read(path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The size of the box around all meshes.
fn size(meshes: &[Mesh]) -> [f32; 3] {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in meshes.iter().flat_map(|m| m.positions.chunks_exact(3).chain(m.lines.chunks_exact(3))) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    [0, 1, 2].map(|k| hi[k] - lo[k])
}

/// A red/green two-triangle object placed twice through components (once mirrored and moved
/// 10 mm), and a blue triangle lifted 1000 mm.
const MODEL: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02">
  <resources>
    <basematerials id="1"><base name="Red" displaycolor="#FF0000"/><base name="Blue" displaycolor="#0000FFFF"/></basematerials>
    <m:colorgroup id="5"><m:color color="#00FF00FF"/></m:colorgroup>
    <object id="2" type="model" pid="1" pindex="0" name="Triangle">
      <mesh>
        <vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="0" y="0" z="1"/></vertices>
        <triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="3" pid="5" p1="0"/></triangles>
      </mesh>
    </object>
    <object id="3" type="model" name="Pair">
      <components>
        <component objectid="2"/>
        <component objectid="2" transform="-1 0 0 0 1 0 0 0 1 10 0 0"/>
      </components>
    </object>
    <object id="4" type="model" pid="1" pindex="1" partnumber="B-1">
      <mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh>
    </object>
  </resources>
  <build><item objectid="3"/><item objectid="4" transform="1 0 0 0 1 0 0 0 1 0 0 1000"/></build>
</model>"##;

#[test]
fn places_components_with_colours_in_metres() {
    let meshes = read(&package(MODEL, &[])).expect("3MF");
    assert_eq!(meshes.len(), 2);
    let pair = &meshes[0];
    assert_eq!(pair.name, "Pair");
    assert_eq!(pair.positions.len() / 9, 4);
    // red and green triangles: vertex colours
    let colors = pair.colors.as_ref().expect("mixed colours");
    assert_eq!(colors[0], [255, 0, 0, 255]);
    assert_eq!(colors[3], [0, 255, 0, 255]);
    // the mirrored copy: x from 9 to 10 mm, still facing up
    let xs: Vec<f32> = pair.positions.chunks_exact(3).map(|p| p[0]).collect();
    assert!(xs.iter().any(|&x| (x - 0.009).abs() < 1e-6) && xs.iter().any(|&x| (x - 0.010).abs() < 1e-6), "{xs:?}");
    assert!(pair.normals.chunks_exact(3).take(3).all(|n| n[2] > 0.99));
    assert!(pair.normals.chunks_exact(3).skip(6).take(3).all(|n| n[2] > 0.99), "mirrored winding");
    let blue = &meshes[1];
    assert_eq!(blue.name, "B-1");
    assert!(blue.colors.is_none());
    assert_eq!(blue.color, [0.0, 0.0, 1.0]);
    assert!((blue.positions[2] - 1.0).abs() < 1e-6, "1000 mm up");
    assert!(blue.props.contains(&("Part number".into(), "B-1".into())));
}

#[test]
fn opens_the_consortium_sphere() {
    let meshes = sample("test-sphere.3mf");
    let triangles: usize = meshes.iter().map(|m| m.positions.len() / 9).sum();
    assert!(triangles > 1000, "{triangles}");
    // a 10 mm radius sphere
    let size = size(&meshes);
    assert!(size.iter().all(|&s| (s - 0.02).abs() < 0.001), "{size:?}");
}

#[test]
fn parses_colours_and_transforms() {
    assert_eq!(parse_color("#11223344"), Some([0x11, 0x22, 0x33, 0x44]));
    assert_eq!(parse_color("red"), None);
    let m = transform(Some("1 0 0 0 1 0 0 0 1 4 5 6"));
    assert_eq!(m.transform_point3(Vec3::ONE), Vec3::new(5.0, 6.0, 7.0));
    assert_eq!(transform(Some("1 2")), Mat4::IDENTITY);
}

#[test]
fn colours_each_corner() {
    // the consortium's pyramid with a colour per corner: red, blue and green all occur
    let meshes = sample("pyramid_vertexcolor.3mf");
    let colors: Vec<[u8; 4]> = meshes.iter().flat_map(|m| m.colors.clone().unwrap_or_default()).collect();
    for c in [[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]] {
        assert!(colors.contains(&c), "{c:?}");
    }
    // and within one triangle the corners differ
    let first = &meshes[0].colors.as_ref().unwrap()[0..3];
    assert!(first[0] != first[1] || first[1] != first[2], "{first:?}");
}

#[test]
fn samples_textures_and_mixes_composites() {
    // the consortium's sphere with a logo texture: several colours from the image
    let meshes = sample("sphere_logo.3mf");
    let colors = meshes[0].colors.as_ref().expect("texture colours");
    let mut distinct: Vec<[u8; 4]> = colors.clone();
    distinct.sort();
    distinct.dedup();
    assert!(distinct.len() > 3, "{}", distinct.len());
    // a composite of half red, half blue base material is purple
    let model = r##"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02">
      <resources>
        <basematerials id="1"><base name="Red" displaycolor="#FF0000"/><base name="Blue" displaycolor="#0000FF"/></basematerials>
        <m:compositematerials id="2" matid="1" matindices="0 1"><m:composite values="0.5 0.5"/></m:compositematerials>
        <m:colorgroup id="3"><m:color color="#FFFFFF80"/></m:colorgroup>
        <m:multiproperties id="4" pids="1 3"><m:multi pindices="0 0"/></m:multiproperties>
        <object id="5" pid="2" pindex="0"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
          <triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="2" v3="1" pid="4" p1="0"/></triangles></mesh></object>
      </resources>
      <build><item objectid="5"/></build>
    </model>"##;
    let meshes = read(&package(model, &[])).unwrap();
    let colors = meshes[0].colors.as_ref().unwrap();
    assert_eq!(colors[0], [128, 0, 128, 255]);
    // red with half-transparent white mixed over it: pink
    assert_eq!(colors[3], [255, 128, 128, 255]);
}

#[test]
fn meshes_beam_lattices() {
    // the consortium's pyramid lattice: beams of a few millimetres with sphere caps, 100 mm high
    let meshes = sample("pyramid.3mf");
    let beams: usize = meshes.iter().filter_map(|m| m.props.iter().find(|(k, _)| k == "Beams")).map(|(_, v)| v.parse::<usize>().unwrap()).sum();
    assert!(beams > 20, "{beams}");
    let size = size(&meshes);
    assert!(size[2] > 0.1 && size[2] < 0.115, "{size:?}");
    // every triangle faces out of its beam: the normals agree with the winding
    for m in &meshes {
        for tri in m.positions.chunks_exact(9).zip(m.normals.chunks_exact(9)) {
            let p = |k: usize| Vec3::new(tri.0[3 * k], tri.0[3 * k + 1], tri.0[3 * k + 2]);
            let n = Vec3::new(tri.1[0] + tri.1[3] + tri.1[6], tri.1[1] + tri.1[4] + tri.1[7], tri.1[2] + tri.1[5] + tri.1[8]);
            assert!((p(1) - p(0)).cross(p(2) - p(0)).dot(n) >= -1e-12);
        }
    }
}

#[test]
fn draws_slices_as_contours() {
    // the consortium's sliced box: its slices are in a part of their own, 0.08 mm apart
    let meshes = sample("box_sliced.3mf");
    let box_ = &meshes[0];
    let slices: usize = box_.props.iter().find(|(k, _)| k == "Slices").map(|(_, v)| v.parse().unwrap()).unwrap();
    assert!(slices > 100, "{slices}");
    assert!(!box_.lines.is_empty());
    let heights: Vec<f32> = box_.lines.chunks_exact(3).map(|p| p[2]).collect();
    let mut levels: Vec<i64> = heights.iter().map(|z| (z * 1e8).round() as i64).collect();
    levels.sort();
    levels.dedup();
    assert_eq!(levels.len(), slices - 1, "one height per slice; the top one is empty");
    assert_eq!(levels[1] - levels[0], 8000, "0.08 mm apart");
    assert!(box_.lines.len() % 6 == 0, "pairs of points");
}

#[test]
fn follows_components_into_other_parts() {
    let meshes = sample("box_prod.3mf");
    assert!(!meshes.is_empty());
    assert!(meshes.iter().all(|m| !m.positions.is_empty()));
}

#[test]
fn reads_the_plates_and_filament_colours_of_a_project() {
    let object = r##"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
      <resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
      <object id="3"><mesh><vertices><vertex x="0" y="0" z="1"/><vertex x="1" y="0" z="1"/><vertex x="0" y="1" z="1"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources>
    </model>"##;
    let root = r##"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06">
      <metadata name="Application">Slicer-1.0</metadata>
      <resources><object id="2"><components><component p:path="/3D/Objects/object_1.model" objectid="1"/><component p:path="/3D/Objects/object_1.model" objectid="3"/></components></object></resources>
      <build><item objectid="2" transform="1 0 0 0 1 0 0 0 1 300 0 0"/></build>
    </model>"##;
    let settings = br#"<?xml version="1.0"?><config>
      <object id="2"><metadata key="name" value="Bracket"/><metadata key="extruder" value="1"/>
        <part id="1" subtype="normal_part"><metadata key="name" value="Body"/></part>
        <part id="3" subtype="normal_part"><metadata key="extruder" value="2"/></part>
      </object>
      <plate><metadata key="plater_id" value="2"/><metadata key="plater_name" value="Left"/><model_instance><metadata key="object_id" value="2"/><metadata key="instance_id" value="0"/></model_instance></plate>
    </config>"#;
    let project = br##"{"filament_colour": ["#FF8000", "#2040FF"], "layer_height": "0.2"}"##;
    let data = package(root, &[("3D/Objects/object_1.model", object.as_bytes()), ("Metadata/model_settings.config", settings), ("Metadata/project_settings.config", project)]);
    let meshes = read(&data).unwrap();
    assert_eq!(meshes.len(), 1);
    let m = &meshes[0];
    assert_eq!(m.name, "Bracket");
    assert!(m.props.contains(&("Plate".into(), "2 (Left)".into())));
    assert!(m.props.contains(&("Made with".into(), "Slicer-1.0".into())));
    let colors = m.colors.as_ref().unwrap();
    assert_eq!(colors[0], [255, 128, 0, 255], "the object's filament");
    assert_eq!(colors[3], [32, 64, 255, 255], "the part's own filament");
}

#[test]
fn colours_volumes_by_their_extruder() {
    let root = r##"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
      <resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
        <triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="2" v3="1"/></triangles></mesh></object></resources>
      <build><item objectid="1"/></build>
    </model>"##;
    let model_config = br#"<?xml version="1.0"?><config><object id="1" instances_count="1"><metadata type="object" key="name" value="Two tone"/>
      <volume firstid="0" lastid="0"><metadata type="volume" key="extruder" value="1"/></volume>
      <volume firstid="1" lastid="1"><metadata type="volume" key="extruder" value="2"/></volume></object></config>"#;
    let config = b"; generated\n; extruder_colour = \"#FF0000\";\"#00FF00\"\n; layer_height = 0.2\n";
    let data = package(root, &[("Metadata/Slic3r_PE_model.config", model_config), ("Metadata/Slic3r_PE.config", config)]);
    let m = &read(&data).unwrap()[0];
    assert_eq!(m.name, "Two tone");
    let colors = m.colors.as_ref().unwrap();
    assert_eq!((colors[0], colors[3]), ([255, 0, 0, 255], [0, 255, 0, 255]));
}

#[test]
fn a_sliced_file_without_a_model_says_so() {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("Metadata/plate_1.gcode", options).unwrap();
    zip.write_all(b"G28\n").unwrap();
    zip.finish().unwrap();
    let error = read(&out.into_inner()).unwrap_err();
    assert!(error.contains("only the G-code of 1 plate"), "{error}");
}
