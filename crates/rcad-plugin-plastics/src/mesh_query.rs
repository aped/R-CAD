//! Mesh queries shared by the plastics analyses (Design Advice, Draft,
//! Accessibility, Minimum Radius): input lookup, the checked per-triangle
//! geometry of a [`BodyMesh`], and ray casting against it through a
//! bounding-volume hierarchy.
//!
//! Errors are plain messages; each analysis prefixes its own name.
//!
//! Tolerances (scale-aware where they are lengths): `diag` is the mesh's
//! bounding-box diagonal; rays start [`REL_OFFSET`]`·diag` off the surface and
//! ignore hits nearer than [`REL_MIN_HIT`]`·diag`; a triangle is degenerate
//! when twice its area is at most `1e-12·diag²`. [`ANGLE_TOL`] bounds
//! "parallel" and "perpendicular" on unit-vector products.
use rcad_plugin::geom::{add, cross, dot, norm, scale, sub};
use rcad_plugin::{AnalysisRequest, BodyMesh, Resolved, Surface};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Angular tolerance (radians, on `n·p` or `|axis × p|`).
pub(crate) const ANGLE_TOL: f64 = 1e-9;
/// Barycentric slack of ray/triangle hits (edges count as hits).
const BARY_TOL: f64 = 1e-10;
/// Ray start offset from the surface, relative to the bounding-box diagonal.
pub(crate) const REL_OFFSET: f64 = 1e-6;
/// Hit-distance floor, relative to the bounding-box diagonal.
pub(crate) const REL_MIN_HIT: f64 = 1e-9;
/// Triangles per BVH leaf.
const LEAF: usize = 4;

pub(crate) fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = norm(v);
    (l.is_finite() && l > 0.0).then(|| scale(v, 1.0 / l))
}

pub(crate) fn pt(v: [f64; 3]) -> Value {
    json!([v[0], v[1], v[2]])
}

/// A finite parameter of the request.
pub(crate) fn param_of(req: &AnalysisRequest, n: &str) -> Result<f64, String> {
    req.params
        .get(n)
        .copied()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("parameter '{n}' is missing or not finite"))
}

/// A choice of the request.
pub(crate) fn choice_of<'a>(req: &'a AnalysisRequest, n: &str) -> Result<&'a str, String> {
    req.choices
        .get(n)
        .map(String::as_str)
        .ok_or_else(|| format!("choice '{n}' is missing"))
}

/// A unit direction: the frame normal of the optional face input `face`,
/// otherwise the axis named by choice `axis` (`+x`, `-x`, ... `-z`), with its
/// source (`"face"` or `"axis"`).
pub(crate) fn direction_of(
    req: &AnalysisRequest,
    face: &str,
    axis: &str,
) -> Result<([f64; 3], &'static str), String> {
    match req.inputs.get(face) {
        Some(Resolved::Face(f)) => unit(f.frame.z)
            .map(|p| (p, "face"))
            .ok_or_else(|| format!("the {face} face's normal is not a direction")),
        Some(_) => Err(format!("input '{face}' must be a face")),
        None => {
            let a = choice_of(req, axis)?;
            let p = match a {
                "+x" => [1.0, 0.0, 0.0],
                "-x" => [-1.0, 0.0, 0.0],
                "+y" => [0.0, 1.0, 0.0],
                "-y" => [0.0, -1.0, 0.0],
                "+z" => [0.0, 0.0, 1.0],
                "-z" => [0.0, 0.0, -1.0],
                a => return Err(format!("unknown {} '{a}'", axis.replace('_', " "))),
            };
            Ok((p, "axis"))
        }
    }
}

/// The mesh of the body named by the Body input `body`.
pub(crate) fn body_mesh(req: &AnalysisRequest) -> Result<&BodyMesh, String> {
    let body = match req.inputs.get("body") {
        Some(Resolved::Body(b)) => b.body,
        Some(_) => return Err("input 'body' must be a body".into()),
        None => return Err("input 'body' is missing".into()),
    };
    req.bodies
        .iter()
        .find(|m| m.body == body)
        .ok_or_else(|| format!("no mesh of body {body}"))
}

/// A checked mesh with per-triangle geometry.
pub(crate) struct Prepared<'a> {
    pub m: &'a BodyMesh,
    pub tri: Vec<[usize; 3]>,
    pub face: Vec<usize>,
    /// Unit normal; zero where `!ok`.
    pub normal: Vec<[f64; 3]>,
    pub area: Vec<f64>,
    pub centroid: Vec<[f64; 3]>,
    /// Non-degenerate.
    pub ok: Vec<bool>,
    pub diag: f64,
    /// Every edge shared by exactly two oppositely wound triangles.
    pub closed: bool,
    /// Undirected edge -> the triangles on it.
    pub edges: HashMap<(usize, usize), Vec<usize>>,
}

impl Prepared<'_> {
    pub fn vert(&self, t: usize) -> [[f64; 3]; 3] {
        self.tri[t].map(|v| self.m.vertices[v])
    }

    /// Triangle indices that are not degenerate.
    pub fn good(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.tri.len()).filter(|&t| self.ok[t])
    }

    pub fn degenerate(&self) -> usize {
        self.ok.iter().filter(|&&g| !g).count()
    }

    /// Total area of the non-degenerate triangles.
    pub fn mesh_area(&self) -> f64 {
        self.good().map(|t| self.area[t]).sum()
    }

    /// The `mesh` block of a report.
    pub fn mesh_json(&self) -> Value {
        json!({
            "triangles": self.tri.len(),
            "degenerate": self.degenerate(),
            "closed": self.closed,
            "area": self.mesh_area(),
        })
    }

    /// Edges shared by exactly two triangles, in a deterministic order.
    pub fn manifold_edges(&self) -> Vec<((usize, usize), [usize; 2])> {
        let mut v: Vec<((usize, usize), [usize; 2])> = self
            .edges
            .iter()
            .filter(|(_, ts)| ts.len() == 2)
            .map(|(e, ts)| (*e, [ts[0], ts[1]]))
            .collect();
        v.sort_unstable_by_key(|(e, _)| *e);
        v
    }

    /// Refuse plane faces whose triangles point against the plane's outward
    /// normal (by more than 60°).
    pub fn check_planes(&self) -> Result<(), String> {
        for t in self.good() {
            if let Surface::Plane { normal } = self.m.faces[self.face[t]].surface
                && unit(normal).is_some_and(|n| dot(n, self.normal[t]) < 0.5)
            {
                return Err(format!(
                    "triangle {t} disagrees with the outward normal of plane face {}",
                    self.face[t]
                ));
            }
        }
        Ok(())
    }
}

/// Check the mesh (finite, index-consistent, outward oriented by its signed
/// volume) and compute per-triangle geometry.
pub(crate) fn prepare(m: &BodyMesh) -> Result<Prepared<'_>, String> {
    let b = m.body;
    if m.triangles.is_empty() || m.vertices.is_empty() {
        return Err(format!("body {b}: empty mesh"));
    }
    if m.tri_face.len() != m.triangles.len() {
        return Err(format!(
            "body {b}: {} face indices for {} triangles",
            m.tri_face.len(),
            m.triangles.len()
        ));
    }
    if !(m.chord.is_finite() && m.chord >= 0.0) {
        return Err(format!("body {b}: chord tolerance {}", m.chord));
    }
    if let Some(i) = m
        .faces
        .iter()
        .position(|f| !(f.area.is_finite() && f.area >= 0.0))
    {
        return Err(format!("body {b}: face {i} has area {}", m.faces[i].area));
    }
    if m.vertices.iter().flatten().any(|v| !v.is_finite()) {
        return Err(format!("body {b}: non-finite vertex"));
    }
    let nv = m.vertices.len();
    let mut tri = Vec::with_capacity(m.triangles.len());
    for (i, t) in m.triangles.iter().enumerate() {
        let t = t.map(|v| v as usize);
        if t.iter().any(|&v| v >= nv) {
            return Err(format!("body {b}: triangle {i} has a vertex out of range"));
        }
        tri.push(t);
    }
    let mut face = Vec::with_capacity(m.tri_face.len());
    for (i, &f) in m.tri_face.iter().enumerate() {
        let f = f as usize;
        if f >= m.faces.len() {
            return Err(format!("body {b}: triangle {i} lies on missing face {f}"));
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
        return Err(format!("body {b}: mesh has no extent"));
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
        return Err(format!("body {b}: every triangle is degenerate"));
    }
    if volume <= 0.0 {
        return Err(format!(
            "body {b}: mesh is not an outward-oriented closed solid (signed volume {volume})"
        ));
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
pub(crate) struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
}

impl Bvh {
    pub fn new(pm: &Prepared) -> Bvh {
        let order: Vec<usize> = pm.good().collect();
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
    pub fn cast(
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

    /// Whether the ray from triangle `t`'s centroid, offset off the surface
    /// along its normal, travels along `d` without hitting any triangle.
    pub fn unobstructed(&self, pm: &Prepared, t: usize, d: [f64; 3]) -> bool {
        let o = add(pm.centroid[t], scale(pm.normal[t], REL_OFFSET * pm.diag));
        self.cast(pm, o, d, REL_MIN_HIT * pm.diag, t, true, |_| true)
            .is_none()
    }
}

pub(crate) fn surface_name(s: &Surface) -> &'static str {
    match s {
        Surface::Plane { .. } => "plane",
        Surface::Cylinder { .. } => "cylinder",
        Surface::Cone { .. } => "cone",
        Surface::Other => "other",
    }
}
