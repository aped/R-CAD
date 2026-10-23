//! Viewport colour map of a plugin analysis report (docs/UI.md "Plugins and
//! the PLASTIC tab"): the report's per-triangle arrays mapped to colours on
//! the analysed body's triangles. The triangles come from the engine's
//! [`AnalysisMesh`] (the host's tessellation call and chord, so the same
//! triangle order); a count that does not match the report's arrays makes
//! the map unavailable rather than colouring the wrong triangles.
//!
//! A derived display only: painted over the 3D view (back faces culled,
//! far to near), never stored, and dropped when the model changes.

use eframe::egui::{self, Color32};
use serde_json::Value;

use crate::engine::AnalysisMesh;

/// Classes of a colour map, in legend order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Draft at or above the minimum / accessible.
    Good,
    /// Negative draft / blocked (an undercut) / radius below the minimum.
    Bad,
    /// Draft below the minimum either way (vertical walls).
    Insufficient,
    /// Faces only the untested direction, or seen edge-on.
    Away,
}

impl Class {
    pub fn colour(self) -> Color32 {
        match self {
            Class::Good => Color32::from_rgb(60, 180, 75),
            Class::Bad => Color32::from_rgb(220, 50, 47),
            Class::Insufficient => Color32::from_rgb(235, 200, 40),
            Class::Away => Color32::from_rgb(150, 150, 150),
        }
    }
}

/// What a report's triangle arrays say, one entry per triangle (None: not
/// coloured, e.g. a degenerate triangle or a radius above the minimum).
#[derive(Debug, Clone, PartialEq)]
pub struct Classified {
    pub classes: Vec<Option<Class>>,
    /// Legend: (class, label).
    pub legend: Vec<(Class, &'static str)>,
}

/// Classify the report's per-triangle arrays: `triangles.class` (draft:
/// positive / negative / insufficient), else `triangles.status`
/// (accessibility: accessible / blocked / away / vertical), else
/// `triangles.radius` against the report's `min_radius` (radii below it).
/// None when the report has none of them.
pub fn classify(rep: &Value) -> Option<Classified> {
    let tri = rep.get("triangles")?.as_object()?;
    if let Some(v) = tri.get("class").and_then(Value::as_array) {
        return Some(Classified {
            classes: v
                .iter()
                .map(|c| match c.as_str()? {
                    "positive" => Some(Class::Good),
                    "negative" => Some(Class::Bad),
                    "insufficient" => Some(Class::Insufficient),
                    _ => None,
                })
                .collect(),
            legend: vec![
                (Class::Good, "positive draft"),
                (Class::Bad, "negative draft"),
                (Class::Insufficient, "insufficient draft"),
            ],
        });
    }
    if let Some(v) = tri.get("status").and_then(Value::as_array) {
        return Some(Classified {
            classes: v
                .iter()
                .map(|c| match c.as_str()? {
                    "accessible" => Some(Class::Good),
                    "blocked" => Some(Class::Bad),
                    "away" | "vertical" => Some(Class::Away),
                    _ => None,
                })
                .collect(),
            legend: vec![
                (Class::Good, "accessible"),
                (Class::Bad, "blocked (undercut)"),
                (Class::Away, "facing away or edge-on"),
            ],
        });
    }
    if let Some(v) = tri.get("radius").and_then(Value::as_array) {
        let min = rep.get("min_radius").and_then(Value::as_f64)?;
        return Some(Classified {
            classes: v
                .iter()
                .map(|r| r.as_f64().filter(|r| *r < min).map(|_| Class::Bad))
                .collect(),
            legend: vec![(Class::Bad, "radius below the minimum")],
        });
    }
    None
}

/// The colour map shown on the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ColourMap {
    /// The document state it was made for (dropped when that changes).
    pub doc_id: u64,
    pub revision: u64,
    /// Analysis name (the legend's title).
    pub name: String,
    /// Triangles of the analysed body's tessellation (the report's count).
    pub triangles: usize,
    /// Coloured triangles as drawn (all placements of the body).
    pub tris: Vec<([[f32; 3]; 3], Class)>,
    pub legend: Vec<(Class, &'static str)>,
}

impl ColourMap {
    /// The map of `rep` on `mesh`, or why it is unavailable.
    pub fn build(
        rep: &Value,
        mesh: Option<&AnalysisMesh>,
        name: &str,
        doc_id: u64,
        revision: u64,
    ) -> Result<Self, String> {
        let c = classify(rep).ok_or("the report has no per-triangle results")?;
        let mesh = mesh.ok_or("the analysed body could not be tessellated for display")?;
        let reported = rep
            .get("mesh")
            .and_then(|m| m.get("triangles"))
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok());
        if c.classes.len() != mesh.triangles || reported.is_some_and(|n| n != mesh.triangles) {
            return Err(format!(
                "the report has {} triangle results but the body tessellates to {} \
                 triangles at {} mm",
                c.classes.len(),
                mesh.triangles,
                mesh.chord
            ));
        }
        let mut tris = Vec::new();
        for placed in &mesh.placed {
            if placed.len() != mesh.triangles {
                return Err("a placement of the body has a different tessellation".into());
            }
            for (t, k) in placed.iter().zip(&c.classes) {
                if let Some(k) = k {
                    tris.push((*t, *k));
                }
            }
        }
        Ok(Self {
            doc_id,
            revision,
            name: name.to_string(),
            triangles: mesh.triangles,
            tris,
            legend: c.legend,
        })
    }

    /// Coloured triangles per class (all placements).
    pub fn count(&self, class: Class) -> usize {
        self.tris.iter().filter(|(_, k)| *k == class).count()
    }
}

/// Opacity of the painted colours (the shaded body shows through a little).
const ALPHA: u8 = 200;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Paint `map` over the 3D view in `rect`: triangles facing the camera
/// (outward winding), sorted far to near so nearer faces cover farther
/// ones. Triangles behind the eye are skipped.
pub fn paint(
    painter: &egui::Painter,
    cam: &rcad_viewport::Camera,
    [w, h]: [f32; 2],
    rect: egui::Rect,
    map: &ColourMap,
) {
    let eye = cam.eye();
    let (_, _, fwd) = cam.basis();
    let mut vis: Vec<(f32, [egui::Pos2; 3], Class)> = Vec::new();
    for (t, k) in &map.tris {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        let c = [
            (t[0][0] + t[1][0] + t[2][0]) / 3.0,
            (t[0][1] + t[1][1] + t[2][1]) / 3.0,
            (t[0][2] + t[1][2] + t[2][2]) / 3.0,
        ];
        if dot(n, sub(eye, c)) <= 0.0 {
            continue;
        }
        let p = t.map(|v| cam.project(v, w, h));
        let (Some(a), Some(b), Some(d)) = (p[0], p[1], p[2]) else {
            continue;
        };
        let at = |q: [f32; 2]| rect.min + egui::vec2(q[0], q[1]);
        vis.push((dot(sub(c, eye), fwd), [at(a), at(b), at(d)], *k));
    }
    vis.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut mesh = egui::Mesh::default();
    for (_, p, k) in vis {
        let col = k.colour();
        let col = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), ALPHA);
        let base = mesh.vertices.len() as u32;
        for q in p {
            mesh.colored_vertex(q, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        // egui meshes take u32 indices; start a new mesh well before that.
        if mesh.vertices.len() > 3_000_000 {
            painter.add(egui::Shape::mesh(std::mem::take(&mut mesh)));
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

impl crate::app::RcadApp {
    /// The colour map shown on the model (tests, the analysis window).
    pub fn colour_map(&self) -> Option<&ColourMap> {
        self.plugins.colour_map.as_ref()
    }

    /// Paint the colour map and its legend over the 3D view.
    pub(crate) fn paint_colour_map(&self, painter: &egui::Painter, rect: egui::Rect) {
        let Some(map) = &self.plugins.colour_map else {
            return;
        };
        let (cam, size) = self.view_camera();
        paint(painter, &cam, size, rect, map);
        let mut at = egui::pos2(rect.min.x + 10.0, rect.max.y - 40.0);
        for (k, text) in map.legend.iter().rev() {
            let r = crate::view3d::chip(
                painter,
                at - egui::vec2(0.0, 22.0),
                format!("■ {text}"),
                k.colour(),
            );
            at.y = r.min.y - 2.0;
        }
        crate::view3d::chip(
            painter,
            at - egui::vec2(0.0, 22.0),
            map.name.clone(),
            crate::theme::of_ctx(painter.ctx()).chip_text,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mesh(n: usize) -> AnalysisMesh {
        AnalysisMesh {
            body: rcad_document::BodyId(1),
            chord: 0.05,
            triangles: n,
            placed: vec![vec![[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]; n]],
        }
    }

    #[test]
    fn draft_classes_map_to_colours() {
        let rep = json!({"triangles": {"class": ["positive", "negative", "insufficient", null]}});
        let m = ColourMap::build(&rep, Some(&mesh(4)), "Draft", 1, 1).unwrap();
        assert_eq!(m.tris.len(), 3);
        assert_eq!(m.count(Class::Insufficient), 1);
    }

    #[test]
    fn radius_below_the_minimum_only() {
        let rep = json!({"min_radius": 1.0, "triangles": {"radius": [0.5, 2.0, null]}});
        let m = ColourMap::build(&rep, Some(&mesh(3)), "Radius", 1, 1).unwrap();
        assert_eq!(m.count(Class::Bad), 1);
        assert_eq!(m.tris.len(), 1);
    }

    #[test]
    fn a_count_mismatch_is_unavailable() {
        let rep = json!({"triangles": {"status": ["accessible", "away"]}});
        let e = ColourMap::build(&rep, Some(&mesh(3)), "Access", 1, 1).unwrap_err();
        assert!(e.contains("2 triangle results"), "{e}");
        assert!(ColourMap::build(&json!({}), Some(&mesh(3)), "x", 1, 1).is_err());
    }
}
