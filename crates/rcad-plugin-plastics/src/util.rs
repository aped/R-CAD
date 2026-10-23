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

/// Closed profiles of a sketch: its circles, and its lines and arcs chained
/// end to end into closed loops (each curve used once; a loop of lines only
/// is a polygon). Open chains are refused with the reason.
pub fn closed_profiles(f: &str, s: &rcad_plugin::SketchGeom) -> Result<Vec<Profile>, PluginError> {
    use rcad_plugin::{Curve, Segment};
    // (start, end, Some((centre, ccw from start to end)) for arcs).
    type Piece = ([f64; 2], [f64; 2], Option<[f64; 2]>);
    let mut out = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    for c in &s.curves {
        match *c {
            Curve::Circle { center, radius } => out.push(Profile::Circle { center, radius }),
            Curve::Line { a, b } => pieces.push((a, b, None)),
            Curve::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let p = |t: f64| [center[0] + radius * t.cos(), center[1] + radius * t.sin()];
                pieces.push((p(start), p(end), Some(center)));
            }
        }
    }
    let same = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < EPS;
    let mut used = vec![false; pieces.len()];
    for first in 0..pieces.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut segs = Vec::new();
        let seg = |pc: &Piece, forward: bool| {
            let from = if forward { pc.0 } else { pc.1 };
            match pc.2 {
                None => Segment::Line { start: from },
                Some(center) => Segment::Arc {
                    start: from,
                    center,
                    ccw: forward,
                },
            }
        };
        segs.push(seg(&pieces[first], true));
        let start = pieces[first].0;
        let mut at = pieces[first].1;
        while !same(at, start) {
            let Some(j) = (0..pieces.len())
                .find(|&j| !used[j] && (same(pieces[j].0, at) || same(pieces[j].1, at)))
            else {
                return Err(invalid(f, "the sketch curves do not form closed loops"));
            };
            used[j] = true;
            let forward = same(pieces[j].0, at);
            segs.push(seg(&pieces[j], forward));
            at = if forward { pieces[j].1 } else { pieces[j].0 };
        }
        if segs.len() < 2
            || (segs.len() < 3 && segs.iter().all(|s| matches!(s, Segment::Line { .. })))
        {
            return Err(invalid(
                f,
                "a closed loop needs at least three lines (or lines and arcs)",
            ));
        }
        out.push(Profile::Loop { segments: segs });
    }
    if out.is_empty() {
        return Err(invalid(f, "the sketch has no closed profile"));
    }
    Ok(out)
}
