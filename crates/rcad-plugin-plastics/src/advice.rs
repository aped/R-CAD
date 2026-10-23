//! Design Advice (docs/PLASTICS.md "Design advice"): wall thickness,
//! thickness variation, draft, undercuts and knife edges of one body for a
//! pull direction, from the exact face kinds plus a closed tessellation
//! ([`rcad_plugin::BodyMesh`]). Read-only; the report is JSON. Autodesk
//! Fusion's Design Advice is the behavioural reference (Thickness, Undercuts,
//! Draft and Knife Edges tabs).
//!
//! # Inputs
//!
//! `body` (a Body) selects the analysed [`BodyMesh`] by id. The pull
//! direction `p` (unit) is the frame normal of the optional `pull` face,
//! otherwise the `pull_axis` choice (`+z`, `-z`, `+x`, ...). The mesh must be
//! finite, index-consistent and outward oriented (positive signed volume);
//! triangles of planar faces must agree with the plane's outward normal.
//! Zero-area triangles are skipped (counted in `mesh.degenerate`).
//!
//! # Semantics
//!
//! Every triangle `t` has its geometric unit normal `n` (from the winding),
//! area `A` and centroid `c`. `diag` is the body's bounding-box diagonal;
//! rays start `1e-6·diag` off the surface and ignore hits nearer than
//! `1e-9·diag` (scale-aware, far below any moulded feature size).
//!
//! - **Thickness** of `t`: the distance from `c` along `−n` (into the
//!   material) to the nearest other triangle through which the ray leaves the
//!   material (hit normal · ray > 0). Ray/triangle tests are Möller–Trumbore
//!   with inclusive edges, accelerated by a bounding-volume hierarchy. A
//!   triangle without such a hit (open mesh) is `unresolved`. Too thin:
//!   thickness < `min_thickness`; too thick: > `max_thickness`. The mean is
//!   area weighted.
//! - **Variation**: two triangles sharing a mesh edge whose normals differ by
//!   at most 30° (so the two sides of a sharp edge, measured in different
//!   directions, are not compared) and whose thicknesses `a ≤ b` satisfy
//!   `b − a > variation · a`. The value is the relative variation
//!   `(b − a)/a`, located at the shared edge's midpoint.
//! - **Draft** of a surface direction `n`: the angle between the surface and
//!   the pull axis, `asin(|n·p|)` (= 90° − acos(|n·p|)) in degrees: 0° for a
//!   wall parallel to the pull, 90° for a face square to it. Side: `+pull`
//!   when `n·p > 0`, `-pull` when `< 0`, `vertical` when `|n·p| ≤ 1e-9`,
//!   `mixed` for a curved face spanning several. Exact (from the face's
//!   surface): planes; cylinders whose axis is parallel to the pull (0°,
//!   vertical); cones whose axis is parallel to the pull (the half-angle; the
//!   side from the mesh). Every other face takes the minimum over its
//!   triangles (approximate). A face is flagged when its draft is below
//!   `min_draft_deg`; the flagged area of an exact face is its exact area, of
//!   a meshed face the area of its triangles below the minimum.
//! - **Undercut**: a two-plate mould opens along `±p`. The `+p` half releases
//!   `t` when `n·p ≥ −1e-9` (the surface does not face away from that half)
//!   and the ray from `c` (offset along `n`) along `+p` hits nothing; the
//!   `−p` half likewise with `n·p ≤ 1e-9` and the ray along `−p`. `t` is an
//!   undercut when neither half releases it (judged at the centroid, so a
//!   triangle partly in an undercut counts wholly in or out).
//! - **Knife edge**: thickness < `knife_edge` (thin material, typically at a
//!   sharp edge or a feathered wall).
//!
//! Mesh-derived numbers approximate the exact body within the tessellation's
//! chord tolerance (reported in `approximation`); exact draft values are
//! marked `exact: true`. The report layout is documented on [`analyze`].
use crate::{choice, input, param};
use rcad_plugin::geom::{add, cross, dot, norm, scale, sub};
use rcad_plugin::schema::AnalysisSchema;
use rcad_plugin::{
    AnalysisRequest, BodyMesh, InputKind, ParamKind, PluginError, Resolved, Surface,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

pub fn schema() -> AnalysisSchema {
    AnalysisSchema {
        id: "design_advice".into(),
        name: "Design Advice".into(),
        description: "Wall thickness, draft, undercuts and knife edges for a pull direction".into(),
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
                "min_thickness",
                "Minimum thickness",
                ParamKind::Length,
                1.0,
                Some((0.0, true)),
                None,
                Some("TMIN"),
            ),
            param(
                "max_thickness",
                "Maximum thickness",
                ParamKind::Length,
                3.0,
                Some((0.0, true)),
                None,
                Some("TMAX"),
            ),
            param(
                "variation",
                "Allowed thickness variation",
                ParamKind::Ratio,
                0.15,
                Some((0.0, false)),
                None,
                Some("TV"),
            ),
            param(
                "min_draft_deg",
                "Minimum draft",
                ParamKind::Angle,
                0.5,
                Some((0.0, false)),
                Some(45.0),
                Some("MDA"),
            ),
            param(
                "knife_edge",
                "Knife edge threshold",
                ParamKind::Length,
                1.0,
                Some((0.0, true)),
                None,
                Some("KE"),
            ),
        ],
        choices: vec![choice(
            "pull_axis",
            "Pull axis",
            &["+z", "-z", "+x", "-x", "+y", "-y"],
        )],
    }
}

const NAME: &str = "design advice";
/// Locations listed per check.
const MAX_LOCATIONS: usize = 10;
/// Angular tolerance (radians, on `n·p` or `|axis × p|`) of "parallel to the
/// pull", of the vertical side and of the release test.
const ANGLE_TOL: f64 = 1e-9;
/// Barycentric slack of ray/triangle hits (edges count as hits).
const BARY_TOL: f64 = 1e-10;
/// Ray start offset from the surface, relative to the bounding-box diagonal.
const REL_OFFSET: f64 = 1e-6;
/// Hit-distance floor, relative to the bounding-box diagonal.
const REL_MIN_HIT: f64 = 1e-9;
/// Largest bend (degrees) between adjacent triangles compared for variation.
const VARIATION_MAX_BEND_DEG: f64 = 30.0;
/// Triangles per BVH leaf.
const LEAF: usize = 4;

fn bad(m: impl std::fmt::Display) -> PluginError {
    PluginError::Input(format!("{NAME}: {m}"))
}

struct Params {
    min_thickness: f64,
    max_thickness: f64,
    variation: f64,
    min_draft_deg: f64,
    knife_edge: f64,
}

fn param_of(req: &AnalysisRequest, n: &str) -> Result<f64, PluginError> {
    req.params
        .get(n)
        .copied()
        .filter(|v| v.is_finite())
        .ok_or_else(|| bad(format!("parameter '{n}' is missing or not finite")))
}

fn params(req: &AnalysisRequest) -> Result<Params, PluginError> {
    let p = Params {
        min_thickness: param_of(req, "min_thickness")?,
        max_thickness: param_of(req, "max_thickness")?,
        variation: param_of(req, "variation")?,
        min_draft_deg: param_of(req, "min_draft_deg")?,
        knife_edge: param_of(req, "knife_edge")?,
    };
    if p.min_thickness < 0.0 || p.max_thickness < 0.0 || p.knife_edge < 0.0 {
        return Err(bad("thickness limits must not be negative"));
    }
    if p.variation <= 0.0 {
        return Err(bad("variation must be positive"));
    }
    if !(p.min_draft_deg > 0.0 && p.min_draft_deg <= 90.0) {
        return Err(bad("minimum draft must be in (0°, 90°]"));
    }
    Ok(p)
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = norm(v);
    (l.is_finite() && l > 0.0).then(|| scale(v, 1.0 / l))
}

fn pull_of(req: &AnalysisRequest) -> Result<([f64; 3], &'static str), PluginError> {
    match req.inputs.get("pull") {
        Some(Resolved::Face(f)) => unit(f.frame.z)
            .map(|p| (p, "face"))
            .ok_or_else(|| bad("the pull face's normal is not a direction")),
        Some(_) => Err(bad("input 'pull' must be a face")),
        None => {
            let axis = req
                .choices
                .get("pull_axis")
                .ok_or_else(|| bad("choice 'pull_axis' is missing"))?;
            let p = match axis.as_str() {
                "+x" => [1.0, 0.0, 0.0],
                "-x" => [-1.0, 0.0, 0.0],
                "+y" => [0.0, 1.0, 0.0],
                "-y" => [0.0, -1.0, 0.0],
                "+z" => [0.0, 0.0, 1.0],
                "-z" => [0.0, 0.0, -1.0],
                a => return Err(bad(format!("unknown pull axis '{a}'"))),
            };
            Ok((p, "axis"))
        }
    }
}

/// A checked mesh with per-triangle geometry.
struct Prepared<'a> {
    m: &'a BodyMesh,
    tri: Vec<[usize; 3]>,
    face: Vec<usize>,
    /// Unit normal; zero where `!ok`.
    normal: Vec<[f64; 3]>,
    area: Vec<f64>,
    centroid: Vec<[f64; 3]>,
    /// Non-degenerate.
    ok: Vec<bool>,
    diag: f64,
    /// Every edge shared by exactly two oppositely wound triangles.
    closed: bool,
    /// Undirected edge -> the triangles on it.
    edges: HashMap<(usize, usize), Vec<usize>>,
}

impl Prepared<'_> {
    fn vert(&self, t: usize) -> [[f64; 3]; 3] {
        self.tri[t].map(|v| self.m.vertices[v])
    }
}

fn prepare(m: &BodyMesh) -> Result<Prepared<'_>, PluginError> {
    let b = m.body;
    if m.triangles.is_empty() || m.vertices.is_empty() {
        return Err(bad(format!("body {b}: empty mesh")));
    }
    if m.tri_face.len() != m.triangles.len() {
        return Err(bad(format!(
            "body {b}: {} face indices for {} triangles",
            m.tri_face.len(),
            m.triangles.len()
        )));
    }
    if !(m.chord.is_finite() && m.chord >= 0.0) {
        return Err(bad(format!("body {b}: chord tolerance {}", m.chord)));
    }
    if let Some(i) = m
        .faces
        .iter()
        .position(|f| !(f.area.is_finite() && f.area >= 0.0))
    {
        return Err(bad(format!(
            "body {b}: face {i} has area {}",
            m.faces[i].area
        )));
    }
    if m.vertices.iter().flatten().any(|v| !v.is_finite()) {
        return Err(bad(format!("body {b}: non-finite vertex")));
    }
    let nv = m.vertices.len();
    let mut tri = Vec::with_capacity(m.triangles.len());
    for (i, t) in m.triangles.iter().enumerate() {
        let t = t.map(|v| v as usize);
        if t.iter().any(|&v| v >= nv) {
            return Err(bad(format!(
                "body {b}: triangle {i} has a vertex out of range"
            )));
        }
        tri.push(t);
    }
    let mut face = Vec::with_capacity(m.tri_face.len());
    for (i, &f) in m.tri_face.iter().enumerate() {
        let f = f as usize;
        if f >= m.faces.len() {
            return Err(bad(format!(
                "body {b}: triangle {i} lies on missing face {f}"
            )));
        }
        face.push(f);
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in &m.vertices {
        for a in 0..3 {
            lo[a] = lo[a].min(v[a]);
            hi[a] = hi[a].max(v[a]);
        }
    }
    let diag = norm(sub(hi, lo));
    if !(diag.is_finite() && diag > 0.0) {
        return Err(bad(format!("body {b}: mesh has no extent")));
    }
    let n = tri.len();
    let mut normal = Vec::with_capacity(n);
    let mut area = Vec::with_capacity(n);
    let mut centroid = Vec::with_capacity(n);
    let mut ok = Vec::with_capacity(n);
    let mut volume = 0.0;
    for t in &tri {
        let [p0, p1, p2] = t.map(|v| m.vertices[v]);
        let c = cross(sub(p1, p0), sub(p2, p0));
        let l = norm(c);
        let good = l > 1e-12 * diag * diag;
        normal.push(if good { scale(c, 1.0 / l) } else { [0.0; 3] });
        area.push(if good { 0.5 * l } else { 0.0 });
        centroid.push(scale(add(add(p0, p1), p2), 1.0 / 3.0));
        ok.push(good);
        let (q0, q1, q2) = (sub(p0, lo), sub(p1, lo), sub(p2, lo));
        volume += dot(q0, cross(q1, q2)) / 6.0;
    }
    if !ok.iter().any(|&g| g) {
        return Err(bad(format!("body {b}: every triangle is degenerate")));
    }
    if volume <= 0.0 {
        return Err(bad(format!(
            "body {b}: mesh is not an outward-oriented closed solid (signed volume {volume})"
        )));
    }
    let mut edges: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    let mut winding: HashMap<(usize, usize), i64> = HashMap::new();
    for (i, t) in tri.iter().enumerate() {
        for k in 0..3 {
            let (a, c) = (t[k], t[(k + 1) % 3]);
            if a == c {
                continue;
            }
            let key = (a.min(c), a.max(c));
            edges.entry(key).or_default().push(i);
            *winding.entry(key).or_default() += if a < c { 1 } else { -1 };
        }
    }
    let closed = edges
        .iter()
        .all(|(e, ts)| ts.len() == 2 && winding.get(e) == Some(&0));
    Ok(Prepared {
        m,
        tri,
        face,
        normal,
        area,
        centroid,
        ok,
        diag,
        closed,
        edges,
    })
}

/// Möller–Trumbore: distance along unit `d` from `o` to the triangle, edges
/// included; None when parallel or missed.
fn tri_hit(o: [f64; 3], d: [f64; 3], [v0, v1, v2]: [[f64; 3]; 3]) -> Option<f64> {
    let e1 = sub(v1, v0);
    let e2 = sub(v2, v0);
    let pv = cross(d, e2);
    let det = dot(e1, pv);
    if det.abs() <= 1e-14 * norm(e1) * norm(e2) {
        return None;
    }
    let inv = 1.0 / det;
    let tv = sub(o, v0);
    let u = dot(tv, pv) * inv;
    if !(-BARY_TOL..=1.0 + BARY_TOL).contains(&u) {
        return None;
    }
    let qv = cross(tv, e1);
    let v = dot(d, qv) * inv;
    if v < -BARY_TOL || u + v > 1.0 + BARY_TOL {
        return None;
    }
    Some(dot(e2, qv) * inv)
}

/// Entry distance (≥ 0) of the ray into the box, None when it misses.
fn slab(lo: [f64; 3], hi: [f64; 3], o: [f64; 3], d: [f64; 3]) -> Option<f64> {
    let (mut tn, mut tf) = (f64::NEG_INFINITY, f64::INFINITY);
    for a in 0..3 {
        if d[a] == 0.0 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d[a];
        let (t0, t1) = ((lo[a] - o[a]) * inv, (hi[a] - o[a]) * inv);
        let (t0, t1) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
        tn = tn.max(t0);
        tf = tf.min(t1);
        if tn > tf {
            return None;
        }
    }
    (tf >= 0.0).then_some(tn.max(0.0))
}

#[derive(Clone, Copy)]
struct Node {
    lo: [f64; 3],
    hi: [f64; 3],
    /// Leaf: first index into `order`; inner node: index of the first child
    /// (the second follows it).
    first: usize,
    /// Triangles of a leaf; 0 for an inner node.
    count: usize,
}

/// Bounding-volume hierarchy over the non-degenerate triangles: median split
/// on the widest centroid axis, so the depth is at most log2(n) + 1.
struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
}

impl Bvh {
    fn new(pm: &Prepared) -> Bvh {
        let order: Vec<usize> = (0..pm.tri.len()).filter(|&t| pm.ok[t]).collect();
        let mut b = Bvh {
            nodes: Vec::with_capacity(2 * order.len() / LEAF + 2),
            order,
        };
        if !b.order.is_empty() {
            b.nodes.push(Node {
                lo: [0.0; 3],
                hi: [0.0; 3],
                first: 0,
                count: 0,
            });
            let n = b.order.len();
            b.split(pm, 0, 0, n, REL_MIN_HIT * pm.diag);
        }
        b
    }

    fn split(&mut self, pm: &Prepared, node: usize, start: usize, end: usize, pad: f64) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut clo = [f64::INFINITY; 3];
        let mut chi = [f64::NEG_INFINITY; 3];
        for &t in &self.order[start..end] {
            for v in pm.vert(t) {
                for a in 0..3 {
                    lo[a] = lo[a].min(v[a]);
                    hi[a] = hi[a].max(v[a]);
                }
            }
            let c = pm.centroid[t];
            for a in 0..3 {
                clo[a] = clo[a].min(c[a]);
                chi[a] = chi[a].max(c[a]);
            }
        }
        self.nodes[node].lo = lo.map(|v| v - pad);
        self.nodes[node].hi = hi.map(|v| v + pad);
        let ext = [chi[0] - clo[0], chi[1] - clo[1], chi[2] - clo[2]];
        let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] {
            0
        } else if ext[1] >= ext[2] {
            1
        } else {
            2
        };
        if end - start <= LEAF || ext[axis] <= 0.0 {
            self.nodes[node].first = start;
            self.nodes[node].count = end - start;
            return;
        }
        let mid = start + (end - start) / 2;
        let cen = &pm.centroid;
        self.order[start..end]
            .select_nth_unstable_by(mid - start, |&a, &b| cen[a][axis].total_cmp(&cen[b][axis]));
        let child = self.nodes.len();
        let blank = self.nodes[node];
        self.nodes.push(blank);
        self.nodes.push(blank);
        self.nodes[node].first = child;
        self.nodes[node].count = 0;
        self.split(pm, child, start, mid, pad);
        self.split(pm, child + 1, mid, end, pad);
    }

    /// The nearest (with `any`: some) triangle other than `exclude` hit
    /// beyond `tmin` whose index `accept`s, with the hit distance.
    #[allow(clippy::too_many_arguments)]
    fn cast(
        &self,
        pm: &Prepared,
        o: [f64; 3],
        d: [f64; 3],
        tmin: f64,
        exclude: usize,
        any: bool,
        accept: impl Fn(usize) -> bool,
    ) -> Option<(f64, usize)> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best = f64::INFINITY;
        let mut hit = None;
        let mut stack = Vec::with_capacity(64);
        stack.push(0usize);
        while let Some(n) = stack.pop() {
            let node = self.nodes[n];
            match slab(node.lo, node.hi, o, d) {
                Some(tn) if tn <= best => {}
                _ => continue,
            }
            if node.count == 0 {
                stack.push(node.first);
                stack.push(node.first + 1);
                continue;
            }
            for &t in &self.order[node.first..node.first + node.count] {
                if t == exclude {
                    continue;
                }
                if let Some(h) = tri_hit(o, d, pm.vert(t))
                    && h > tmin
                    && h < best
                    && accept(t)
                {
                    best = h;
                    hit = Some((h, t));
                    if any {
                        return hit;
                    }
                }
            }
        }
        hit
    }
}

fn side_of(d: f64) -> &'static str {
    if d.abs() <= ANGLE_TOL {
        "vertical"
    } else if d > 0.0 {
        "+pull"
    } else {
        "-pull"
    }
}

fn draft_deg(d: f64) -> f64 {
    d.abs().min(1.0).asin().to_degrees()
}

fn surface_name(s: &Surface) -> &'static str {
    match s {
        Surface::Plane { .. } => "plane",
        Surface::Cylinder { .. } => "cylinder",
        Surface::Cone { .. } => "cone",
        Surface::Other => "other",
    }
}

/// Exact draft of a face (degrees, and the side when the surface fixes it),
/// or None when it has to come from the mesh.
type ExactDraft = Option<(f64, Option<&'static str>)>;

fn exact_draft(s: &Surface, p: [f64; 3], fi: usize) -> Result<ExactDraft, PluginError> {
    match *s {
        Surface::Plane { normal } => {
            let n = unit(normal).ok_or_else(|| bad(format!("face {fi}: invalid plane normal")))?;
            let d = dot(n, p);
            Ok(Some((draft_deg(d), Some(side_of(d)))))
        }
        Surface::Cylinder { axis, .. } => {
            let a = unit(axis).ok_or_else(|| bad(format!("face {fi}: invalid cylinder axis")))?;
            Ok((norm(cross(a, p)) <= ANGLE_TOL).then_some((0.0, Some("vertical"))))
        }
        Surface::Cone {
            axis, half_angle, ..
        } => {
            let a = unit(axis).ok_or_else(|| bad(format!("face {fi}: invalid cone axis")))?;
            if !(half_angle.is_finite()
                && half_angle > 0.0
                && half_angle < std::f64::consts::FRAC_PI_2)
            {
                return Err(bad(format!("face {fi}: cone half-angle {half_angle}")));
            }
            Ok((norm(cross(a, p)) <= ANGLE_TOL).then_some((half_angle.to_degrees(), None)))
        }
        Surface::Other => Ok(None),
    }
}

/// A finding at a triangle: (value, triangle).
type At = (f64, usize);

fn keep_min(slot: &mut Option<At>, v: At) {
    if slot.is_none_or(|s| v.0 < s.0) {
        *slot = Some(v);
    }
}

fn keep_max(slot: &mut Option<At>, v: At) {
    if slot.is_none_or(|s| v.0 > s.0) {
        *slot = Some(v);
    }
}

/// Per-face accumulation of every check.
#[derive(Default)]
struct FaceAcc {
    thin: Option<At>,
    thick: Option<At>,
    t_min: Option<At>,
    t_max: Option<At>,
    /// (relative variation, location).
    variation: Option<(f64, [f64; 3])>,
    undercut_area: f64,
    undercut_at: Option<At>,
    knife: Option<At>,
    draft_tri: Option<At>,
    draft_area_mesh: f64,
    largest: Option<At>,
    /// Sides seen among the triangles: +pull, -pull, vertical.
    sides: [bool; 3],
}

fn pt(v: [f64; 3]) -> Value {
    json!([v[0], v[1], v[2]])
}

/// Sorted, truncated location list of triangle findings.
fn locations(pm: &Prepared, mut v: Vec<At>, ascending: bool) -> Value {
    v.sort_by(|a, b| {
        let o = a.0.total_cmp(&b.0);
        (if ascending { o } else { o.reverse() }).then(a.1.cmp(&b.1))
    });
    v.truncate(MAX_LOCATIONS);
    Value::Array(
        v.into_iter()
            .map(|(val, t)| {
                json!({"face": pm.face[t], "value": val, "location": pt(pm.centroid[t])})
            })
            .collect(),
    )
}

/// Run Design Advice (semantics in the module documentation). The report,
/// lengths in mm, areas in mm², angles in degrees, locations world points:
///
/// ```text
/// body, pull [x,y,z], pull_source "face"|"axis",
/// approximation {chord, note},
/// mesh {triangles, degenerate, closed, area},
/// summary {min_thickness, max_thickness, mean_thickness, too_thin_area,
///          too_thick_area, variation_area, draft_area, undercut_area,
///          knife_edge_area, alerts {kind: count}},
/// thickness {min, max, mean, too_thin_area, too_thick_area, unresolved,
///            faces [{face, min, max}], thinnest [loc], thickest [loc]},
/// variation {pairs, area, max,
///            locations [{face, value, location, thickness [a, b]}]},
/// draft {min_draft_deg, area_below,
///        faces [{face, surface, draft_deg, area_below, side, exact}]},
/// undercut {area, triangles, faces [{face, area}], locations [loc]},
/// knife_edge {triangles, area, locations [loc]},
/// alerts [{kind, face, value, unit, location, message}]
/// ```
///
/// `loc` is `{face, value, location}` (at most 10, worst first). Alert kinds
/// are `too_thin`, `too_thick`, `variation`, `draft`, `undercut` and
/// `knife_edge`, one per face and kind at its worst point, ordered by face
/// then kind; `unit` is `mm`, `ratio`, `deg` or `mm2`. Values that do not
/// exist (no resolved thickness) are null.
pub fn analyze(req: &AnalysisRequest) -> Result<Value, PluginError> {
    let prm = params(req)?;
    let body = match req.inputs.get("body") {
        Some(Resolved::Body(b)) => b.body,
        Some(_) => return Err(bad("input 'body' must be a body")),
        None => return Err(bad("input 'body' is missing")),
    };
    let mesh = req
        .bodies
        .iter()
        .find(|m| m.body == body)
        .ok_or_else(|| bad(format!("no mesh of body {body}")))?;
    let (p, pull_source) = pull_of(req)?;
    let pm = prepare(mesh)?;
    let nt = pm.tri.len();
    let nf = mesh.faces.len();

    // Exact face drafts, and the orientation check of planar faces.
    let mut exact = Vec::with_capacity(nf);
    for (fi, f) in mesh.faces.iter().enumerate() {
        exact.push(exact_draft(&f.surface, p, fi)?);
    }
    for t in (0..nt).filter(|&t| pm.ok[t]) {
        if let Surface::Plane { normal } = mesh.faces[pm.face[t]].surface
            && unit(normal).is_some_and(|n| dot(n, pm.normal[t]) < 0.5)
        {
            return Err(bad(format!(
                "triangle {t} disagrees with the outward normal of plane face {}",
                pm.face[t]
            )));
        }
    }

    let bvh = Bvh::new(&pm);
    let off = REL_OFFSET * pm.diag;
    let tiny = REL_MIN_HIT * pm.diag;

    // Thickness: inward ray to the nearest exit.
    let thickness: Vec<Option<f64>> = (0..nt)
        .map(|t| {
            if !pm.ok[t] {
                return None;
            }
            let d = scale(pm.normal[t], -1.0);
            bvh.cast(&pm, pm.centroid[t], d, tiny, t, false, |h| {
                dot(pm.normal[h], d) > 0.0
            })
            .map(|(h, _)| h)
        })
        .collect();

    let mut acc: Vec<FaceAcc> = (0..nf).map(|_| FaceAcc::default()).collect();
    let (mut t_min, mut t_max): (Option<f64>, Option<f64>) = (None, None);
    let (mut sum_at, mut sum_a) = (0.0, 0.0);
    let (mut thin_area, mut thick_area, mut knife_area) = (0.0, 0.0, 0.0);
    let mut unresolved = 0usize;
    let mut resolved: Vec<At> = Vec::new();
    let mut knives: Vec<At> = Vec::new();
    let mut mesh_area = 0.0;
    for t in (0..nt).filter(|&t| pm.ok[t]) {
        let a = pm.area[t];
        let fa = &mut acc[pm.face[t]];
        mesh_area += a;
        keep_max(&mut fa.largest, (a, t));
        let Some(th) = thickness[t] else {
            unresolved += 1;
            continue;
        };
        resolved.push((th, t));
        t_min = Some(t_min.map_or(th, |m| m.min(th)));
        t_max = Some(t_max.map_or(th, |m| m.max(th)));
        sum_at += a * th;
        sum_a += a;
        keep_min(&mut fa.t_min, (th, t));
        keep_max(&mut fa.t_max, (th, t));
        if th < prm.min_thickness {
            thin_area += a;
            keep_min(&mut fa.thin, (th, t));
        }
        if th > prm.max_thickness {
            thick_area += a;
            keep_max(&mut fa.thick, (th, t));
        }
        if th < prm.knife_edge {
            knife_area += a;
            knives.push((th, t));
            keep_min(&mut fa.knife, (th, t));
        }
    }
    let mean = (sum_a > 0.0).then(|| sum_at / sum_a);

    // Variation across smooth shared edges, in a deterministic edge order.
    let cos_bend = VARIATION_MAX_BEND_DEG.to_radians().cos();
    let mut edge_list: Vec<((usize, usize), [usize; 2])> = pm
        .edges
        .iter()
        .filter(|(_, ts)| ts.len() == 2)
        .map(|(e, ts)| (*e, [ts[0], ts[1]]))
        .collect();
    edge_list.sort_unstable_by_key(|(e, _)| *e);
    let mut var_pairs: Vec<(f64, usize, usize, [f64; 3])> = Vec::new();
    let mut var_tri = vec![false; nt];
    for ((va, vb), [ta, tb]) in edge_list {
        let (Some(a), Some(b)) = (thickness[ta], thickness[tb]) else {
            continue;
        };
        if dot(pm.normal[ta], pm.normal[tb]) < cos_bend {
            continue;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        if lo <= 0.0 || hi - lo <= prm.variation * lo {
            continue;
        }
        let rel = (hi - lo) / lo;
        let mid = scale(add(mesh.vertices[va], mesh.vertices[vb]), 0.5);
        var_tri[ta] = true;
        var_tri[tb] = true;
        for t in [ta, tb] {
            let fa = &mut acc[pm.face[t]];
            if fa.variation.is_none_or(|v| rel > v.0) {
                fa.variation = Some((rel, mid));
            }
        }
        var_pairs.push((rel, ta, tb, mid));
    }
    let var_area: f64 = (0..nt).filter(|&t| var_tri[t]).map(|t| pm.area[t]).sum();
    let var_max = var_pairs
        .iter()
        .map(|v| v.0)
        .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
    var_pairs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let var_locations: Vec<Value> = var_pairs
        .iter()
        .take(MAX_LOCATIONS)
        .map(|&(rel, ta, tb, mid)| {
            json!({"face": pm.face[ta], "value": rel, "location": pt(mid),
                   "thickness": [thickness[ta], thickness[tb]]})
        })
        .collect();

    // Draft and undercuts per triangle.
    let mut undercuts: Vec<At> = Vec::new();
    let mut undercut_area = 0.0;
    let neg_p = scale(p, -1.0);
    for t in (0..nt).filter(|&t| pm.ok[t]) {
        let n = pm.normal[t];
        let d = dot(n, p);
        let fa = &mut acc[pm.face[t]];
        let dd = draft_deg(d);
        keep_min(&mut fa.draft_tri, (dd, t));
        if dd < prm.min_draft_deg {
            fa.draft_area_mesh += pm.area[t];
        }
        let side = match side_of(d) {
            "+pull" => 0,
            "-pull" => 1,
            _ => 2,
        };
        fa.sides[side] = true;
        let o = add(pm.centroid[t], scale(n, off));
        let free = |dir: [f64; 3]| bvh.cast(&pm, o, dir, tiny, t, true, |_| true).is_none();
        let released = (d >= -ANGLE_TOL && free(p)) || (d <= ANGLE_TOL && free(neg_p));
        if !released {
            undercut_area += pm.area[t];
            fa.undercut_area += pm.area[t];
            keep_max(&mut fa.undercut_at, (pm.area[t], t));
            undercuts.push((pm.area[t], t));
        }
    }

    // Per-face draft.
    let mut draft_faces = Vec::new();
    let mut draft_area = 0.0;
    let mut face_draft: Vec<Option<(f64, Option<usize>)>> = vec![None; nf];
    for (fi, fa) in acc.iter().enumerate() {
        let mesh_side = match fa.sides {
            [true, false, false] => "+pull",
            [false, true, false] => "-pull",
            [false, false, true] => "vertical",
            [false, false, false] => "none",
            _ => "mixed",
        };
        let (deg, side, is_exact, at) = match exact[fi] {
            Some((deg, side)) => (
                deg,
                side.unwrap_or(mesh_side),
                true,
                fa.largest.map(|l| l.1),
            ),
            None => match fa.draft_tri {
                Some((deg, t)) => (deg, mesh_side, false, Some(t)),
                None => continue,
            },
        };
        let below = match (is_exact, deg < prm.min_draft_deg) {
            (true, true) => mesh.faces[fi].area,
            (true, false) => 0.0,
            (false, _) => fa.draft_area_mesh,
        };
        draft_area += below;
        if deg < prm.min_draft_deg {
            face_draft[fi] = Some((deg, at));
        }
        draft_faces.push(json!({
            "face": fi,
            "surface": surface_name(&mesh.faces[fi].surface),
            "draft_deg": deg,
            "area_below": below,
            "side": side,
            "exact": is_exact,
        }));
    }

    // Alerts: one per face and kind, at the worst point.
    let loc_of = |t: Option<usize>| t.map_or(Value::Null, |t| pt(pm.centroid[t]));
    let mut alerts = Vec::new();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut alert = |kind: &'static str, face: usize, value: f64, unit: &str, loc, msg: String| {
        *counts.entry(kind).or_default() += 1;
        alerts.push(
            json!({"kind": kind, "face": face, "value": value, "unit": unit,
                           "location": loc, "message": msg}),
        );
    };
    for (fi, fa) in acc.iter().enumerate() {
        if let Some((v, t)) = fa.thin {
            let m = format!(
                "face {fi}: wall {v:.3} mm is thinner than the minimum {} mm",
                prm.min_thickness
            );
            alert("too_thin", fi, v, "mm", loc_of(Some(t)), m);
        }
        if let Some((v, t)) = fa.thick {
            let m = format!(
                "face {fi}: wall {v:.3} mm is thicker than the maximum {} mm",
                prm.max_thickness
            );
            alert("too_thick", fi, v, "mm", loc_of(Some(t)), m);
        }
        if let Some((v, at)) = fa.variation {
            let m = format!(
                "face {fi}: thickness changes by {:.1} % across an edge (allowed {:.1} %)",
                v * 100.0,
                prm.variation * 100.0
            );
            alert("variation", fi, v, "ratio", pt(at), m);
        }
        if let Some((v, at)) = face_draft[fi] {
            let m = format!(
                "face {fi}: draft {v:.3}° is below the minimum {}°",
                prm.min_draft_deg
            );
            alert("draft", fi, v, "deg", loc_of(at), m);
        }
        if fa.undercut_area > 0.0 {
            let v = fa.undercut_area;
            let m = format!("face {fi}: {v:.3} mm² cannot release in either pull direction");
            alert(
                "undercut",
                fi,
                v,
                "mm2",
                loc_of(fa.undercut_at.map(|u| u.1)),
                m,
            );
        }
        if let Some((v, t)) = fa.knife {
            let m = format!(
                "face {fi}: knife edge {v:.3} mm thick (threshold {} mm)",
                prm.knife_edge
            );
            alert("knife_edge", fi, v, "mm", loc_of(Some(t)), m);
        }
    }

    let thickness_faces: Vec<Value> = acc
        .iter()
        .enumerate()
        .filter_map(|(fi, fa)| Some(json!({"face": fi, "min": fa.t_min?.0, "max": fa.t_max?.0})))
        .collect();
    let undercut_faces: Vec<Value> = acc
        .iter()
        .enumerate()
        .filter(|(_, fa)| fa.undercut_area > 0.0)
        .map(|(fi, fa)| json!({"face": fi, "area": fa.undercut_area}))
        .collect();
    let degenerate = pm.ok.iter().filter(|&&g| !g).count();
    let (n_undercut, n_knife) = (undercuts.len(), knives.len());

    Ok(json!({
        "body": body,
        "pull": pt(p),
        "pull_source": pull_source,
        "approximation": {
            "chord": mesh.chord,
            "note": format!(
                "approximate: thickness, variation, undercuts, knife edges and the draft of \
                 faces with exact: false come from the tessellation within {} mm; draft with \
                 exact: true is computed from the exact surface",
                mesh.chord
            ),
        },
        "mesh": {
            "triangles": nt,
            "degenerate": degenerate,
            "closed": pm.closed,
            "area": mesh_area,
        },
        "summary": {
            "min_thickness": t_min,
            "max_thickness": t_max,
            "mean_thickness": mean,
            "too_thin_area": thin_area,
            "too_thick_area": thick_area,
            "variation_area": var_area,
            "draft_area": draft_area,
            "undercut_area": undercut_area,
            "knife_edge_area": knife_area,
            "alerts": counts,
        },
        "thickness": {
            "min": t_min,
            "max": t_max,
            "mean": mean,
            "too_thin_area": thin_area,
            "too_thick_area": thick_area,
            "unresolved": unresolved,
            "faces": thickness_faces,
            "thinnest": locations(&pm, resolved.clone(), true),
            "thickest": locations(&pm, resolved, false),
        },
        "variation": {
            "pairs": var_pairs.len(),
            "area": var_area,
            "max": var_max,
            "locations": var_locations,
        },
        "draft": {
            "min_draft_deg": prm.min_draft_deg,
            "area_below": draft_area,
            "faces": draft_faces,
        },
        "undercut": {
            "area": undercut_area,
            "triangles": n_undercut,
            "faces": undercut_faces,
            "locations": locations(&pm, undercuts, false),
        },
        "knife_edge": {
            "triangles": n_knife,
            "area": knife_area,
            "locations": locations(&pm, knives, true),
        },
        "alerts": alerts,
    }))
}
