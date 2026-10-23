//! Shared construction helpers of the plastics features.
use rcad_plugin::geom::{add, cross, scale};
use rcad_plugin::{Frame, Mode, Op, PluginError, Profile};

/// Geometric slack of placement checks (mm), well above the kernel's
/// linear tolerance and far below any plastic feature size.
pub const EPS: f64 = 1e-6;

/// Depth (mm) a tool starting on a face is sunk into the material under it,
/// so its bottom is not coplanar with that face (the overlap is inside
/// material the tool joins anyway; the result check covers it).
pub const SINK: f64 = 0.2;

#[allow(clippy::too_many_arguments)]
pub fn prism(
    id: u32,
    body: &str,
    frame: Frame,
    profile: Profile,
    holes: Vec<Profile>,
    (from, to): (f64, f64),
    taper_deg: f64,
    mode: Mode,
    label: String,
) -> Op {
    Op::Prism {
        id,
        body: body.into(),
        frame,
        profile,
        holes,
        from,
        to,
        taper_deg,
        mode,
        label,
    }
}

/// Frame standing on plane `base` at plane point `p`: `x` the in-plane
/// direction at angle `a` (radians, from `base.x`), `y` the plane normal,
/// `z = x × y` (in the plane). A profile drawn in (x, y) is a section
/// perpendicular to the plane; its span runs along `z`.
pub fn standing(base: &Frame, p: [f64; 2], a: f64) -> Frame {
    let (s, c) = a.sin_cos();
    let x = add(scale(base.x, c), scale(base.y, s));
    let y = base.z;
    Frame {
        origin: base.point(p, 0.0),
        x,
        y,
        z: cross(x, y),
    }
}

pub fn invalid(feature: &str, m: impl std::fmt::Display) -> PluginError {
    PluginError::Input(format!("{feature}: {m}"))
}

pub fn unsupported(feature: &str, m: impl std::fmt::Display) -> PluginError {
    PluginError::Unsupported(format!("{feature}: {m}"))
}

/// Placement points in sketch order (operation ids derive from the
/// position in this list, so it must not depend on coordinates):
/// standalone sketch points, else circle centres; repeats dropped.
pub fn placement_points(s: &rcad_plugin::SketchGeom) -> Vec<[f64; 2]> {
    let mut v: Vec<[f64; 2]> = if s.points.is_empty() {
        s.curves
            .iter()
            .filter_map(|c| match c {
                rcad_plugin::Curve::Circle { center, .. } => Some(*center),
                _ => None,
            })
            .collect()
    } else {
        s.points.clone()
    };
    let mut out: Vec<[f64; 2]> = Vec::new();
    for p in v.drain(..) {
        if !out.iter().any(|q| (p[0] - q[0]).hypot(p[1] - q[1]) < EPS) {
            out.push(p);
        }
    }
    out
}
