//! Inspect analyses for moulded parts:
//! Draft Analysis, Accessibility and Minimum Radius of one body, from the
//! exact face kinds plus a closed tessellation ([`rcad_plugin::BodyMesh`]).
//! Read-only; reports are JSON with per-face results, summary totals and
//! per-triangle arrays (indexed like `BodyMesh::triangles`, `null` for
//! degenerate triangles) for viewport colouring.
//!
//! The mesh is checked as for Design Advice ([`crate::advice`]): finite,
//! index-consistent, outward oriented, plane triangles agreeing with their
//! plane's outward normal. Every triangle has its unit normal `n` (from the
//! winding), area and centroid. Areas are triangle areas (the tessellation's
//! approximation of the face areas, within its chord tolerance).
//!
//! # Draft Analysis (`draft_analysis`)
//!
//! The signed draft of a surface direction `n` for the pull `p` (unit) is
//! `asin(n·p)` in degrees: positive when the surface opens towards `+p`
//! (released by the `+p` mould half), negative towards `−p`, 0° for a wall
//! parallel to the pull, ±90° square to it. Classes with the minimum draft
//! `m` (`min_draft_deg`): `positive` (≥ m), `negative` (≤ −m), `insufficient`
//! (between; includes vertical walls), compared with a 1e-9° slack. A face's
//! value is exact for planes, cylinders coaxial with the pull (0°) and cones
//! coaxial with the pull (± the half-angle; the sign from the face's
//! triangles, which must agree); every other face reports the minimum and
//! maximum over its triangles and the class `mixed` when they span several.
//! Triangles of exact faces carry the exact value. `max_draft_deg` sets the
//! colour range `[−max, max]` (the draft-angle range); values beyond it
//! are reported unclamped.
//!
//! # Accessibility (`accessibility`)
//!
//! A triangle is accessible from direction `d` when it faces `d`
//! (`n·d > 1e-9`) and the ray from its centroid (offset off the surface)
//! along `d` hits nothing: a straight tool or mould half moving along `−d`
//! reaches it. With `both_directions = both`, accessible from `+d` or `−d`
//! (a two-plate mould). Inaccessible triangles are `blocked` (they face a
//! tested direction but something is in the way: an undercut), `away`
//! (they face only the untested direction) or `vertical` (`|n·d| ≤ 1e-9`,
//! seen edge-on). Judged at the centroid: a triangle counts wholly one way.
//!
//! # Minimum Radius (`minimum_radius`)
//!
//! The smallest principal radius of curvature per face. Planes: none
//! (infinite, skipped). Cylinders: the exact radius, concave for holes.
//! Cones: `ρ / cos(half-angle)` (the circumferential radius) at the smallest
//! distance `ρ` from the axis among the face's mesh vertices (exact surface,
//! sampled at the vertices; 0 at an apex). Other faces: estimated from the
//! mesh at each edge shared by two triangles of the face as
//! `(h_a + h_b) / θ`, `θ` the angle between the two normals and `h_a`, `h_b`
//! the signed in-plane distances of the triangles' circumcentres from the
//! edge (the arc length across the edge over the turn of the normal; a facet
//! with its vertices on a sphere or cylinder has its normal through its
//! circumcentre); edges with `θ ≤ 1e-9` are flat and edges with
//! `h_a + h_b ≤ 0` (very obtuse slivers) give no estimate. With vertices on
//! the surface the estimate is low by about `β²/6` (`β` the facets' angular
//! size), so it errs towards flagging; irregular triangulations scatter it
//! further.
//! Faces whose radius is below `min_radius` are flagged; sharp edges between
//! faces are not reported.
use crate::mesh_query::{
    ANGLE_TOL, Bvh, Prepared, body_mesh, choice_of, direction_of, param_of, prepare, pt,
    surface_name, unit,
};
use crate::{choice, input, param};
use rcad_plugin::geom::{add, cross, dot, norm, scale, sub};
use rcad_plugin::schema::AnalysisSchema;
use rcad_plugin::{AnalysisRequest, InputKind, ParamKind, PluginError, Surface};
use serde_json::{Value, json};

const AXES: &[&str] = &["+z", "-z", "+x", "-x", "+y", "-y"];
/// Slack (degrees) of the draft class boundaries.
const DEG_TOL: f64 = 1e-9;

pub fn draft_schema() -> AnalysisSchema {
    AnalysisSchema {
        id: "draft_analysis".into(),
        name: "Draft Analysis".into(),
        description: "Signed draft of every face for a pull direction: positive, negative or \
                      insufficient"
            .into(),
        inputs: vec![
            input("body", "Body", InputKind::Body, false),
            input(
                "pull",
                "Pull direction (face normal)",
                InputKind::Face,
                true,
            ),
        ],
        params: vec![
            param(
                "min_draft_deg",
                "Minimum draft",
                ParamKind::Angle,
                1.0,
                Some((0.0, true)),
                Some(89.0),
                Some("DA"),
            ),
            param(
                "max_draft_deg",
                "Colour range",
                ParamKind::Angle,
                90.0,
                Some((0.0, true)),
                Some(90.0),
                None,
            ),
        ],
        choices: vec![choice("pull_axis", "Pull axis", AXES)],
    }
}

pub fn accessibility_schema() -> AnalysisSchema {
    AnalysisSchema {
        id: "accessibility".into(),
        name: "Accessibility Analysis".into(),
        description: "Faces reachable along a direction by a straight tool or mould half".into(),
        inputs: vec![
            input("body", "Body", InputKind::Body, false),
            input(
                "direction",
                "Direction (face normal)",
                InputKind::Face,
                true,
            ),
        ],
        params: vec![],
        choices: vec![
            choice("direction_axis", "Direction axis", AXES),
            choice("both_directions", "Directions", &["one", "both"]),
        ],
    }
}

pub fn minimum_radius_schema() -> AnalysisSchema {
    AnalysisSchema {
        id: "minimum_radius".into(),
        name: "Minimum Radius Analysis".into(),
        description: "Smallest curvature radius of every face, flagged below a minimum".into(),
        inputs: vec![input("body", "Body", InputKind::Body, false)],
        params: vec![param(
            "min_radius",
            "Minimum radius",
            ParamKind::Length,
            1.0,
            Some((0.0, true)),
            None,
            Some("NR"),
        )],
        choices: vec![],
    }
}

fn err(name: &str) -> impl Fn(String) -> PluginError + '_ {
    move |m| PluginError::Input(format!("{name}: {m}"))
}

/// The checked mesh of the request's body.
fn load<'a>(req: &'a AnalysisRequest, name: &str) -> Result<Prepared<'a>, PluginError> {
    let bad = err(name);
    let pm = prepare(body_mesh(req).map_err(&bad)?).map_err(&bad)?;
    pm.check_planes().map_err(&bad)?;
    Ok(pm)
}

fn approximation(pm: &Prepared, what: &str) -> Value {
    json!({
        "chord": pm.m.chord,
        "note": format!(
            "approximate: {what} come from the tessellation within {} mm",
            pm.m.chord
        ),
    })
}

fn opt(v: Option<f64>) -> Value {
    v.map_or(Value::Null, Value::from)
}

fn signed_draft_deg(s: f64) -> f64 {
    s.clamp(-1.0, 1.0).asin().to_degrees()
}

const CLASSES: [&str; 3] = ["positive", "negative", "insufficient"];

fn draft_class(v: f64, min: f64) -> usize {
    if v >= min - DEG_TOL {
        0
    } else if v <= -min + DEG_TOL {
        1
    } else {
        2
    }
}

/// Exact signed draft of face `fi`, or None when it comes from the mesh.
fn exact_signed_draft(pm: &Prepared, fi: usize, p: [f64; 3]) -> Result<Option<f64>, String> {
    match pm.m.faces[fi].surface {
        Surface::Plane { normal } => {
            let n = unit(normal).ok_or_else(|| format!("face {fi}: invalid plane normal"))?;
            Ok(Some(signed_draft_deg(dot(n, p))))
        }
        Surface::Cylinder { axis, radius, .. } => {
            let a = unit(axis).ok_or_else(|| format!("face {fi}: invalid cylinder axis"))?;
            if !(radius.is_finite() && radius > 0.0) {
                return Err(format!("face {fi}: cylinder radius {radius}"));
            }
            Ok((norm(cross(a, p)) <= ANGLE_TOL).then_some(0.0))
        }
        Surface::Cone {
            axis, half_angle, ..
        } => {
            let a = unit(axis).ok_or_else(|| format!("face {fi}: invalid cone axis"))?;
            check_half_angle(fi, half_angle)?;
            if norm(cross(a, p)) > ANGLE_TOL {
                return Ok(None);
            }
            // The side comes from the triangles; all must agree.
            let mut sides = [false; 2];
            for t in pm.good().filter(|&t| pm.face[t] == fi) {
                let s = dot(pm.normal[t], p);
                if s > ANGLE_TOL {
                    sides[0] = true;
                } else if s < -ANGLE_TOL {
                    sides[1] = true;
                } else {
                    sides = [true, true];
                }
            }
            let deg = half_angle.to_degrees();
            Ok(match sides {
                [true, false] => Some(deg),
                [false, true] => Some(-deg),
                _ => None,
            })
        }
        Surface::Other => Ok(None),
    }
}

fn check_half_angle(fi: usize, half_angle: f64) -> Result<(), String> {
    if half_angle.is_finite() && half_angle > 0.0 && half_angle < std::f64::consts::FRAC_PI_2 {
        Ok(())
    } else {
        Err(format!("face {fi}: cone half-angle {half_angle}"))
    }
}

/// Run Draft Analysis (semantics in the module documentation). The report,
/// angles in degrees, areas in mm²:
///
/// ```text
/// body, pull [x,y,z], pull_source "face"|"axis", min_draft_deg,
/// range_deg [-max, max], approximation {chord, note},
/// mesh {triangles, degenerate, closed, area},
/// summary {positive_area, negative_area, insufficient_area, vertical_area,
///          total_area, min_deg, max_deg,
///          faces {positive, negative, insufficient, mixed}},
/// faces [{face, surface, exact, draft_deg (exact value or null), min_deg,
///         max_deg, class "positive"|"negative"|"insufficient"|"mixed",
///         area {positive, negative, insufficient}}],
/// triangles {draft_deg [v|null], class [c|null]}
/// ```
pub fn draft_analysis(req: &AnalysisRequest) -> Result<Value, PluginError> {
    const NAME: &str = "draft analysis";
    let bad = err(NAME);
    let min = param_of(req, "min_draft_deg").map_err(&bad)?;
    let max = param_of(req, "max_draft_deg").map_err(&bad)?;
    if !(min > 0.0 && min < 90.0) {
        return Err(bad("minimum draft must be in (0°, 90°)".into()));
    }
    if !(max > min && max <= 90.0) {
        return Err(bad("colour range must be in (minimum draft, 90°]".into()));
    }
    let (p, source) = direction_of(req, "pull", "pull_axis").map_err(&bad)?;
    let pm = load(req, NAME)?;
    let nf = pm.m.faces.len();
    let mut exact = Vec::with_capacity(nf);
    for fi in 0..nf {
        exact.push(exact_signed_draft(&pm, fi, p).map_err(&bad)?);
    }

    let nt = pm.tri.len();
    let mut tri_deg: Vec<Option<f64>> = vec![None; nt];
    let mut tri_class: Vec<Option<usize>> = vec![None; nt];
    // Per face: class areas, min, max, triangle count.
    let mut fa = vec![([0.0f64; 3], f64::INFINITY, f64::NEG_INFINITY, 0usize); nf];
    let mut totals = [0.0f64; 3];
    let (mut vertical, mut lo, mut hi) = (0.0, f64::INFINITY, f64::NEG_INFINITY);
    let vertical_deg = ANGLE_TOL.to_degrees();
    for t in pm.good() {
        let fi = pm.face[t];
        let v = exact[fi].unwrap_or_else(|| signed_draft_deg(dot(pm.normal[t], p)));
        let c = draft_class(v, min);
        let a = pm.area[t];
        tri_deg[t] = Some(v);
        tri_class[t] = Some(c);
        let f = &mut fa[fi];
        f.0[c] += a;
        f.1 = f.1.min(v);
        f.2 = f.2.max(v);
        f.3 += 1;
        totals[c] += a;
        if v.abs() <= vertical_deg {
            vertical += a;
        }
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let mut face_counts = [0usize; 4];
    let mut faces = Vec::new();
    for (fi, &(area, fmin, fmax, n)) in fa.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let present: Vec<usize> = (0..3).filter(|&c| area[c] > 0.0).collect();
        let (class, k) = match present.as_slice() {
            [c] => (CLASSES[*c], *c),
            _ => ("mixed", 3),
        };
        face_counts[k] += 1;
        faces.push(json!({
            "face": fi,
            "surface": surface_name(&pm.m.faces[fi].surface),
            "exact": exact[fi].is_some(),
            "draft_deg": opt(exact[fi]),
            "min_deg": fmin,
            "max_deg": fmax,
            "class": class,
            "area": {"positive": area[0], "negative": area[1], "insufficient": area[2]},
        }));
    }
    let total: f64 = totals.iter().sum();
    Ok(json!({
        "body": pm.m.body,
        "pull": pt(p),
        "pull_source": source,
        "min_draft_deg": min,
        "range_deg": [-max, max],
        "approximation": approximation(
            &pm,
            "areas, and the draft of faces with exact: false,"
        ),
        "mesh": pm.mesh_json(),
        "summary": {
            "positive_area": totals[0],
            "negative_area": totals[1],
            "insufficient_area": totals[2],
            "vertical_area": vertical,
            "total_area": total,
            "min_deg": lo,
            "max_deg": hi,
            "faces": {
                "positive": face_counts[0],
                "negative": face_counts[1],
                "insufficient": face_counts[2],
                "mixed": face_counts[3],
            },
        },
        "faces": faces,
        "triangles": {
            "draft_deg": tri_deg.iter().map(|v| opt(*v)).collect::<Vec<_>>(),
            "class": tri_class
                .iter()
                .map(|c| c.map_or(Value::Null, |c| CLASSES[c].into()))
                .collect::<Vec<_>>(),
        },
    }))
}

/// Accessibility of one triangle.
#[derive(Clone, Copy, PartialEq)]
enum Access {
    Accessible,
    Blocked,
    Away,
    Vertical,
}

impl Access {
    fn name(self) -> &'static str {
        match self {
            Access::Accessible => "accessible",
            Access::Blocked => "blocked",
            Access::Away => "away",
            Access::Vertical => "vertical",
        }
    }
}

/// Run Accessibility Analysis (semantics in the module documentation). The
/// report, areas in mm²:
///
/// ```text
/// body, direction [x,y,z], direction_source "face"|"axis",
/// directions "one"|"both", approximation {chord, note},
/// mesh {triangles, degenerate, closed, area},
/// summary {accessible_area, inaccessible_area, blocked_area, away_area,
///          vertical_area, total_area,
///          faces {accessible, inaccessible, partial}},
/// faces [{face, status "accessible"|"inaccessible"|"partial",
///         accessible_area, inaccessible_area, blocked_area, away_area,
///         vertical_area}],
/// triangles {accessible [bool|null],
///            status ["accessible"|"blocked"|"away"|"vertical"|null]}
/// ```
pub fn accessibility(req: &AnalysisRequest) -> Result<Value, PluginError> {
    const NAME: &str = "accessibility";
    let bad = err(NAME);
    let (d, source) = direction_of(req, "direction", "direction_axis").map_err(&bad)?;
    let both = match choice_of(req, "both_directions").map_err(&bad)? {
        "one" => false,
        "both" => true,
        v => return Err(bad(format!("unknown directions '{v}' (one or both)"))),
    };
    let pm = load(req, NAME)?;
    let bvh = Bvh::new(&pm);
    let nt = pm.tri.len();
    let nf = pm.m.faces.len();
    let neg_d = scale(d, -1.0);
    let mut status: Vec<Option<Access>> = vec![None; nt];
    // Per face: accessible, blocked, away, vertical areas; triangle count.
    let mut fa = vec![([0.0f64; 4], 0usize); nf];
    for t in pm.good() {
        let s = dot(pm.normal[t], d);
        let a = if s.abs() <= ANGLE_TOL {
            Access::Vertical
        } else if s < 0.0 && !both {
            Access::Away
        } else if bvh.unobstructed(&pm, t, if s > 0.0 { d } else { neg_d }) {
            Access::Accessible
        } else {
            Access::Blocked
        };
        status[t] = Some(a);
        let f = &mut fa[pm.face[t]];
        f.0[a as usize] += pm.area[t];
        f.1 += 1;
    }
    let mut totals = [0.0f64; 4];
    let mut counts = [0usize; 3];
    let mut faces = Vec::new();
    for (fi, &(ar, n)) in fa.iter().enumerate() {
        if n == 0 {
            continue;
        }
        for k in 0..4 {
            totals[k] += ar[k];
        }
        let inacc = ar[1] + ar[2] + ar[3];
        let (st, k) = match (ar[0] > 0.0, inacc > 0.0) {
            (true, false) => ("accessible", 0),
            (false, _) => ("inaccessible", 1),
            (true, true) => ("partial", 2),
        };
        counts[k] += 1;
        faces.push(json!({
            "face": fi,
            "status": st,
            "accessible_area": ar[0],
            "inaccessible_area": inacc,
            "blocked_area": ar[1],
            "away_area": ar[2],
            "vertical_area": ar[3],
        }));
    }
    Ok(json!({
        "body": pm.m.body,
        "direction": pt(d),
        "direction_source": source,
        "directions": if both { "both" } else { "one" },
        "approximation": approximation(&pm, "all results"),
        "mesh": pm.mesh_json(),
        "summary": {
            "accessible_area": totals[0],
            "inaccessible_area": totals[1] + totals[2] + totals[3],
            "blocked_area": totals[1],
            "away_area": totals[2],
            "vertical_area": totals[3],
            "total_area": totals.iter().sum::<f64>(),
            "faces": {"accessible": counts[0], "inaccessible": counts[1], "partial": counts[2]},
        },
        "faces": faces,
        "triangles": {
            "accessible": status
                .iter()
                .map(|s| s.map_or(Value::Null, |s| (s == Access::Accessible).into()))
                .collect::<Vec<_>>(),
            "status": status
                .iter()
                .map(|s| s.map_or(Value::Null, |s| s.name().into()))
                .collect::<Vec<_>>(),
        },
    }))
}

/// Distance of `q` from the line through `o` along unit `a`.
fn axis_distance(q: [f64; 3], o: [f64; 3], a: [f64; 3]) -> f64 {
    norm(cross(sub(q, o), a))
}

/// Signed distance, within triangle `t`'s plane, from the edge line through
/// `p0` along unit `e` to the triangle's circumcentre (positive on the
/// triangle's side). None when the triangle is degenerate.
fn across(pm: &Prepared, t: usize, p0: [f64; 3], e: [f64; 3]) -> Option<f64> {
    let [a, b, c] = pm.vert(t);
    let (u, v) = (sub(b, a), sub(c, a));
    let w = cross(u, v);
    let w2 = dot(w, w);
    if w2 <= 0.0 {
        return None;
    }
    let cc = add(
        a,
        scale(
            add(scale(cross(v, w), dot(u, u)), scale(cross(w, u), dot(v, v))),
            0.5 / w2,
        ),
    );
    // In-plane perpendicular to the edge, towards the vertex off the edge.
    let off = [a, b, c]
        .into_iter()
        .map(|x| sub(x, p0))
        .max_by(|x, y| axis_distance(*x, [0.0; 3], e).total_cmp(&axis_distance(*y, [0.0; 3], e)))?;
    let m = unit(sub(off, scale(e, dot(off, e))))?;
    Some(dot(sub(cc, p0), m))
}

/// Per-face minimum radius: (radius, concave, location).
type Min = Option<(f64, bool, [f64; 3])>;

fn keep(slot: &mut Min, v: (f64, bool, [f64; 3])) {
    if slot.is_none_or(|s| v.0 < s.0) {
        *slot = Some(v);
    }
}

/// Run Minimum Radius Analysis (semantics in the module documentation). The
/// report, lengths in mm, areas in mm²:
///
/// ```text
/// body, min_radius, approximation {chord, note},
/// mesh {triangles, degenerate, closed, area},
/// summary {min_radius (smallest face radius or null), flagged_faces,
///          flagged_area, exact_faces, approximate_faces},
/// faces [{face, surface, method "plane"|"cylinder"|"cone"|"mesh", exact,
///         radius (null: plane or flat), concave (bool|null), below,
///         area_below, location ([x,y,z]|null)}],
/// triangles {radius [r|null]}
/// ```
pub fn minimum_radius(req: &AnalysisRequest) -> Result<Value, PluginError> {
    const NAME: &str = "minimum radius";
    let bad = err(NAME);
    let threshold = param_of(req, "min_radius").map_err(&bad)?;
    if threshold <= 0.0 {
        return Err(bad("minimum radius must be positive".into()));
    }
    let pm = load(req, NAME)?;
    let nt = pm.tri.len();
    let nf = pm.m.faces.len();
    let mut tri_r: Vec<Option<f64>> = vec![None; nt];
    let mut fmin: Vec<Min> = vec![None; nf];
    let mut has_tri = vec![false; nf];
    for t in pm.good() {
        has_tri[pm.face[t]] = true;
    }

    // Exact and sampled surfaces.
    for (fi, f) in pm.m.faces.iter().enumerate() {
        match f.surface {
            Surface::Plane { .. } | Surface::Other => {}
            Surface::Cylinder {
                axis,
                radius,
                outward,
                ..
            } => {
                if unit(axis).is_none() || !(radius.is_finite() && radius > 0.0) {
                    return Err(bad(format!("face {fi}: invalid cylinder")));
                }
                for t in pm.good().filter(|&t| pm.face[t] == fi) {
                    tri_r[t] = Some(radius);
                    keep(&mut fmin[fi], (radius, !outward, pm.centroid[t]));
                }
            }
            Surface::Cone {
                point,
                axis,
                half_angle,
            } => {
                let a = unit(axis).ok_or_else(|| bad(format!("face {fi}: invalid cone axis")))?;
                check_half_angle(fi, half_angle).map_err(&bad)?;
                let cos = half_angle.cos();
                for t in pm.good().filter(|&t| pm.face[t] == fi) {
                    let vs = pm.vert(t);
                    let (rho, at) = vs
                        .iter()
                        .map(|&v| (axis_distance(v, point, a), v))
                        .min_by(|x, y| x.0.total_cmp(&y.0))
                        .unwrap_or((0.0, pm.centroid[t]));
                    let c = pm.centroid[t];
                    let rel = sub(c, point);
                    let radial = sub(rel, scale(a, dot(rel, a)));
                    let concave = dot(pm.normal[t], radial) < 0.0;
                    let r = rho / cos;
                    tri_r[t] = Some(r);
                    keep(&mut fmin[fi], (r, concave, at));
                }
            }
        }
    }

    // Mesh estimates on other faces, at edges inside one face.
    let mesh_face = |fi: usize| matches!(pm.m.faces[fi].surface, Surface::Other);
    for ((va, vb), [ta, tb]) in pm.manifold_edges() {
        let fi = pm.face[ta];
        if pm.face[tb] != fi || !mesh_face(fi) || !pm.ok[ta] || !pm.ok[tb] {
            continue;
        }
        let (na, nb) = (pm.normal[ta], pm.normal[tb]);
        let theta = norm(cross(na, nb)).atan2(dot(na, nb));
        if theta <= ANGLE_TOL {
            continue;
        }
        let (p0, p1) = (pm.m.vertices[va], pm.m.vertices[vb]);
        let Some(e) = unit(sub(p1, p0)) else {
            continue;
        };
        let (Some(ha), Some(hb)) = (across(&pm, ta, p0, e), across(&pm, tb, p0, e)) else {
            continue;
        };
        if ha + hb <= 0.0 {
            continue;
        }
        let r = (ha + hb) / theta;
        let concave = dot(na, sub(pm.centroid[tb], pm.centroid[ta])) > 0.0;
        let mid = scale(add(p0, p1), 0.5);
        for t in [ta, tb] {
            tri_r[t] = Some(tri_r[t].map_or(r, |x: f64| x.min(r)));
        }
        keep(&mut fmin[fi], (r, concave, mid));
    }

    let mut faces = Vec::new();
    let (mut flagged, mut flagged_area, mut n_exact, mut n_approx) = (0usize, 0.0, 0usize, 0usize);
    let mut overall: Option<f64> = None;
    for fi in 0..nf {
        if !has_tri[fi] {
            continue;
        }
        let s = &pm.m.faces[fi].surface;
        let (method, exact) = match s {
            Surface::Plane { .. } => ("plane", true),
            Surface::Cylinder { .. } => ("cylinder", true),
            Surface::Cone { .. } => ("cone", false),
            Surface::Other => ("mesh", false),
        };
        if exact {
            n_exact += 1;
        } else {
            n_approx += 1;
        }
        let below = fmin[fi].is_some_and(|m| m.0 < threshold);
        let area_below: f64 = pm
            .good()
            .filter(|&t| pm.face[t] == fi && tri_r[t].is_some_and(|r| r < threshold))
            .map(|t| pm.area[t])
            .sum();
        if below {
            flagged += 1;
            flagged_area += area_below;
        }
        if let Some(m) = fmin[fi] {
            overall = Some(overall.map_or(m.0, |o| o.min(m.0)));
        }
        faces.push(json!({
            "face": fi,
            "surface": surface_name(s),
            "method": method,
            "exact": exact,
            "radius": opt(fmin[fi].map(|m| m.0)),
            "concave": fmin[fi].map_or(Value::Null, |m| m.1.into()),
            "below": below,
            "area_below": area_below,
            "location": fmin[fi].map_or(Value::Null, |m| pt(m.2)),
        }));
    }
    Ok(json!({
        "body": pm.m.body,
        "min_radius": threshold,
        "approximation": approximation(
            &pm,
            "areas, cone radii (sampled at mesh vertices) and radii with method mesh"
        ),
        "mesh": pm.mesh_json(),
        "summary": {
            "min_radius": opt(overall),
            "flagged_faces": flagged,
            "flagged_area": flagged_area,
            "exact_faces": n_exact,
            "approximate_faces": n_approx,
        },
        "faces": faces,
        "triangles": {"radius": tri_r.iter().map(|r| opt(*r)).collect::<Vec<_>>()},
    }))
}
