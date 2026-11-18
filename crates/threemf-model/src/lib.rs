//! The plain model the 3MF reader returns. It knows nothing of the program that shows it.

/// The grey of a triangle without a colour (sRGB).
pub const DEFAULT_COLOR: [f32; 3] = [0.72, 0.72, 0.74];

/// One build item: its triangles and lines placed in the build, in metres, Z up.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    /// The object's name (as the slicer project names it, else as the model part does), else
    /// its part number, else `Item <n>`.
    pub name: String,
    /// xyz per vertex; three vertices per triangle.
    pub positions: Vec<f32>,
    /// xyz per vertex, unit length.
    pub normals: Vec<f32>,
    /// The colour of the whole item, when all its triangles and lines have one (sRGB, 0..1).
    pub color: [f32; 3],
    /// sRGB colour per vertex (RGBA8), when the triangles differ in colour.
    pub colors: Option<Vec<[u8; 4]>>,
    /// Line segments, two points (xyz) each: the contours of the slices of a sliced object.
    pub lines: Vec<f32>,
    /// sRGB colour per line point (RGBA8), when the colours differ.
    pub line_colors: Option<Vec<[u8; 4]>>,
    /// What the file says of the item: name, object id, part number, plate, beams, slices.
    pub props: Vec<(String, String)>,
}
