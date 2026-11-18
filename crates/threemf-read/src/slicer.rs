//! What the project files of slicers that save their projects as 3MF say of the model: the names
//! of objects and parts, the filament (extruder) each prints with and that filament's colour,
//! and the plate each object stands on. Two layouts are read:
//!
//! - `Metadata/model_settings.config` (objects with their parts and plates, XML) with the
//!   filament colours in `Metadata/project_settings.config` (JSON);
//! - `Metadata/Slic3r_PE_model.config` (objects with their volumes as ranges of triangles, XML)
//!   with the extruder colours in `Metadata/Slic3r_PE.config` (`; key = value` lines).
//!
//! The print settings in these files are not read.

use super::{parse_color, read_entry, Archive};
use std::collections::HashMap;

/// An object of the build as the slicer names and colours it.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ObjectInfo {
    pub name: Option<String>,
    pub color: Option<[u8; 4]>,
}

#[derive(Debug, Default)]
pub(crate) struct Slicer {
    /// The program that wrote the package, as its root part says.
    pub program: Option<String>,
    /// By build object id.
    pub objects: HashMap<u32, ObjectInfo>,
    /// The colour of each part (by component object id) of a build object.
    pub parts: HashMap<u32, HashMap<u32, [u8; 4]>>,
    /// Ranges of triangles (first, last) of a build object that print in a colour.
    pub ranges: HashMap<u32, Vec<(u32, u32, [u8; 4])>>,
    /// The plate (number and name) of a build object.
    plates: HashMap<u32, (u32, String)>,
}

impl Slicer {
    pub fn read(archive: &mut Archive, entries: &[String]) -> Slicer {
        let has = |name: &str| entries.iter().any(|e| e.eq_ignore_ascii_case(name));
        let mut slicer = Slicer::default();
        if has("Metadata/model_settings.config") {
            let colors = read_entry(archive, "Metadata/project_settings.config").ok().map(|t| json_colors(&t)).unwrap_or_default();
            if let Ok(text) = read_entry(archive, "Metadata/model_settings.config") {
                slicer.model_settings(&text, &colors);
            }
        } else if has("Metadata/Slic3r_PE_model.config") {
            let colors = read_entry(archive, "Metadata/Slic3r_PE.config").ok().map(|t| line_colors(&t)).unwrap_or_default();
            if let Ok(text) = read_entry(archive, "Metadata/Slic3r_PE_model.config") {
                slicer.volumes(&text, &colors);
            }
        }
        slicer
    }

    /// The plate an object stands on, as `1` or `1 (name)`.
    pub fn plate_of(&self, object: u32) -> Option<String> {
        let (number, name) = self.plates.get(&object)?;
        Some(if name.is_empty() { number.to_string() } else { format!("{number} ({name})") })
    }

    fn model_settings(&mut self, text: &str, colors: &[[u8; 4]]) {
        let Ok(doc) = roxmltree::Document::parse(text) else { return };
        let meta = |n: roxmltree::Node, key: &str| {
            n.children().filter(|c| c.has_tag_name("metadata")).find(|c| c.attribute("key") == Some(key)).and_then(|c| c.attribute("value")).map(str::to_string)
        };
        let extruder = |n: roxmltree::Node| meta(n, "extruder").and_then(|e| e.parse::<usize>().ok()).filter(|&e| e > 0).and_then(|e| colors.get(e - 1).copied());
        for object in doc.root_element().children().filter(|n| n.has_tag_name("object")) {
            let Some(id) = object.attribute("id").and_then(|v| v.parse().ok()) else { continue };
            let color = extruder(object);
            self.objects.insert(id, ObjectInfo { name: meta(object, "name").filter(|n| !n.is_empty()), color });
            let parts: HashMap<u32, [u8; 4]> = object
                .children()
                .filter(|n| n.has_tag_name("part"))
                .filter_map(|p| Some((p.attribute("id")?.parse().ok()?, extruder(p).or(color)?)))
                .collect();
            if !parts.is_empty() {
                self.parts.insert(id, parts);
            }
        }
        for plate in doc.root_element().children().filter(|n| n.has_tag_name("plate")) {
            let number = meta(plate, "plater_id").and_then(|v| v.parse().ok()).unwrap_or(0);
            let name = meta(plate, "plater_name").unwrap_or_default();
            for instance in plate.children().filter(|n| n.has_tag_name("model_instance")) {
                if let Some(object) = meta(instance, "object_id").and_then(|v| v.parse().ok()) {
                    self.plates.insert(object, (number, name.clone()));
                }
            }
        }
    }

    fn volumes(&mut self, text: &str, colors: &[[u8; 4]]) {
        let Ok(doc) = roxmltree::Document::parse(text) else { return };
        let meta = |n: roxmltree::Node, key: &str| {
            n.children().filter(|c| c.has_tag_name("metadata")).find(|c| c.attribute("key") == Some(key)).and_then(|c| c.attribute("value")).map(str::to_string)
        };
        let extruder = |n: roxmltree::Node| meta(n, "extruder").and_then(|e| e.parse::<usize>().ok()).filter(|&e| e > 0).and_then(|e| colors.get(e - 1).copied());
        for object in doc.root_element().children().filter(|n| n.has_tag_name("object")) {
            let Some(id) = object.attribute("id").and_then(|v| v.parse().ok()) else { continue };
            let color = extruder(object).or(colors.first().copied());
            self.objects.insert(id, ObjectInfo { name: meta(object, "name").filter(|n| !n.is_empty()), color });
            let ranges: Vec<(u32, u32, [u8; 4])> = object
                .children()
                .filter(|n| n.has_tag_name("volume"))
                .filter_map(|v| Some((v.attribute("firstid")?.parse().ok()?, v.attribute("lastid")?.parse().ok()?, extruder(v).or(color)?)))
                .collect();
            if !ranges.is_empty() {
                self.ranges.insert(id, ranges);
            }
        }
    }
}

/// The filament colours of a JSON project settings file (`"filament_colour": ["#RRGGBB", ..]`).
fn json_colors(text: &str) -> Vec<[u8; 4]> {
    let Some(start) = text.find("\"filament_colour\"") else { return Vec::new() };
    let rest = &text[start..];
    let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) else { return Vec::new() };
    if close < open {
        return Vec::new();
    }
    rest[open + 1..close].split(',').filter_map(|v| parse_color(v.trim().trim_matches('"'))).collect()
}

/// The extruder colours of a `; key = value` settings file: `extruder_colour`, where empty the
/// `filament_colour` of the same extruder.
fn line_colors(text: &str) -> Vec<[u8; 4]> {
    let value = |key: &str| -> Vec<Option<[u8; 4]>> {
        text.lines()
            .filter_map(|l| l.trim_start_matches(';').trim().strip_prefix(key)?.trim_start().strip_prefix('=').map(str::trim))
            .next()
            .map(|v| v.split(';').map(|c| parse_color(c.trim().trim_matches('"'))).collect())
            .unwrap_or_default()
    };
    let extruders = value("extruder_colour");
    let filaments = value("filament_colour");
    (0..extruders.len().max(filaments.len())).filter_map(|i| extruders.get(i).copied().flatten().or(filaments.get(i).copied().flatten())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_filament_colours() {
        assert_eq!(json_colors(r##"{"filament_colour": ["#FF0000", "#00FF00"], "x": 1}"##), [[255, 0, 0, 255], [0, 255, 0, 255]]);
        let text = "; extruder_colour = \"\";\"#0000FF\"\n; filament_colour = \"#FFFFFF\";\"#000000\"\n";
        assert_eq!(line_colors(text), [[255, 255, 255, 255], [0, 0, 255, 255]]);
    }
}
