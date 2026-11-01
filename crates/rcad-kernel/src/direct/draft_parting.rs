//! Exact draft of planar faces about a broken (polyline) parting line:
//! Fusion's Parting Line draft with a sketch-curve parting tool made of
//! straight segments.
//!
//! The parting surface is the polyline swept along `across` (the normal of
//! its sketch plane, perpendicular to the pull): one planar strip per
//! segment. With heights measured along the pull, the polyline must be a
//! graph over the horizontal (its points strictly ordered across the pull)
//! and span every selected face.
//!
//! Construction, without booleans (it extends [`super::draft_faces`]):
//! * every selected face is split along its intersection with the parting
//!   surface: new vertices where its straight edges cross it, and a chain of
//!   straight edges through the points where the strips' common lines pierce
//!   the face (the parting line's corners on the face);
//! * every drafted piece (both sides, or the pull side only) turns about each
//!   of its parting segments by the angle, relative to its current
//!   orientation, leaning in going away from the parting line; a piece with
//!   corners becomes one planar facet per segment, the facets meeting in
//!   straight edges from the corners to new vertices on the piece's far edge
//!   (whose neighbour, not drafted, keeps its plane);
//! * every vertex of a changed face becomes the intersection of the planes
//!   meeting there, as in the planar draft (the parting line's points stay:
//!   every plane through them passes through them).
//!
//! Refused by name: arcs (non-planar strips), a parting line that does not
//! cross a selected face once along a single chain, a corner on an edge or
//! outside the face, a segment along the pull (no hinge), corners whose
//! facet edges do not all reach one far edge with an undrafted neighbour,
//! and everything the planar draft refuses.
use super::draft::finish;
use super::*;
use truck_topology::compress::CompressedEdgeIndex as Ix;

/// The parting surface: polyline points as (x across the pull, z along it)
/// about `p0`, swept along `s`.
struct Parting {
    p0: Point3,
    h: Vector3,
    p: Vector3,
    s: Vector3,
    xz: Vec<[f64; 2]>,
}

impl Parting {
    fn x(&self, q: Point3) -> f64 {
        (q - self.p0).dot(self.h)
    }

    /// Parting height at `x` (None outside the polyline's span).
    fn zp(&self, x: f64, tol: f64) -> Option<f64> {
        let n = self.xz.len();
        if x < self.xz[0][0] - tol || x > self.xz[n - 1][0] + tol {
            return None;
        }
        let k = self
            .xz
            .windows(2)
            .position(|w| x <= w[1][0])
            .unwrap_or(n - 2);
        let ([x0, z0], [x1, z1]) = (self.xz[k], self.xz[k + 1]);
        Some(z0 + (z1 - z0) * (x - x0) / (x1 - x0))
    }

    /// Height of `q` above the parting surface (along the pull).
    fn g(&self, q: Point3, tol: f64) -> Result<f64> {
        let z = (q - self.p0).dot(self.p);
        self.zp(self.x(q), tol).map(|zp| z - zp).ok_or_else(|| {
            invalid(
                "draft: the parting line does not span the faces to draft (extend it past them)",
            )
        })
    }

    /// Where segment `a b` crosses the surface (its single sign change).
    fn crossing(&self, a: Point3, b: Point3, tol: f64) -> Result<Point3> {
        // g is piecewise linear along the segment, with kinks where x passes
        // a polyline point.
        let (xa, xb) = (self.x(a), self.x(b));
        let mut ts = vec![0.0, 1.0];
        for &[xk, _] in &self.xz {
            if (xb - xa).abs() > 0.0 {
                let t = (xk - xa) / (xb - xa);
                if t > 0.0 && t < 1.0 {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        let at = |t: f64| a + (b - a) * t;
        let mut roots = Vec::new();
        for w in ts.windows(2) {
            let (g0, g1) = (self.g(at(w[0]), tol)?, self.g(at(w[1]), tol)?);
            if (g0 > 0.0) != (g1 > 0.0) {
                roots.push(w[0] + (w[1] - w[0]) * g0 / (g0 - g1));
            }
        }
        match roots.as_slice() {
            [t] => Ok(at(*t)),
            _ => Err(invalid(format!(
                "draft: an edge crosses the parting line {} times; it must cross once",
                roots.len()
            ))),
        }
    }
}

fn start_of(sh: &CS, ei: &Ix) -> usize {
    let (a, b) = sh.edges[ei.index].vertices;
    if ei.orientation { a } else { b }
}

fn end_of(sh: &CS, ei: &Ix) -> usize {
    let (a, b) = sh.edges[ei.index].vertices;
    if ei.orientation { b } else { a }
}

fn line_edge(sh: &mut CS, a: usize, b: usize) -> usize {
    sh.edges.push(truck_topology::compress::CompressedEdge {
        vertices: (a, b),
        curve: Curve::Line(Line(sh.vertices[a], sh.vertices[b])),
    });
    sh.edges.len() - 1
}

/// Split straight edge `e` at `points` (ordered from its first vertex):
/// new vertices, and every wire using it takes the pieces in order.
fn split_edge(sh: &mut CS, e: usize, points: &[Point3]) -> Vec<usize> {
    let (a, b) = sh.edges[e].vertices;
    let mut verts = vec![a];
    for p in points {
        sh.vertices.push(*p);
        verts.push(sh.vertices.len() - 1);
    }
    verts.push(b);
    let mut pieces = vec![e];
    sh.edges[e].vertices = (verts[0], verts[1]);
    sh.edges[e].curve = Curve::Line(Line(sh.vertices[verts[0]], sh.vertices[verts[1]]));
    for w in verts[1..].windows(2) {
        pieces.push(line_edge(sh, w[0], w[1]));
    }
    for face in sh.faces.iter_mut() {
        for wire in face.boundaries.iter_mut() {
            let mut out = Vec::with_capacity(wire.len() + pieces.len());
            for ei in wire.iter() {
                if ei.index != e {
                    out.push(*ei);
                } else if ei.orientation {
                    out.extend(pieces.iter().map(|&index| Ix {
                        index,
                        orientation: true,
                    }));
                } else {
                    out.extend(pieces.iter().rev().map(|&index| Ix {
                        index,
                        orientation: false,
                    }));
                }
            }
            *wire = out;
        }
    }
    verts[1..verts.len() - 1].to_vec()
}

/// Plane through `c` turned by `angle` about the line along `e` from the
/// plane with outward normal `n`, leaning in going along `dir`.
fn turned(
    c: Point3,
    e: Vector3,
    n: Vector3,
    dir: Vector3,
    angle: f64,
) -> Result<(Point3, Vector3)> {
    let mut t = e.normalize().cross(n);
    if t.dot(dir).abs() < 1e-9 {
        return Err(invalid(
            "draft: a parting segment runs along the pull on a face (no hinge to turn about)",
        ));
    }
    if t.dot(dir) < 0.0 {
        t = -t;
    }
    Ok((c, (n * angle.cos() + t * angle.sin()).normalize()))
}

/// Draft faces `faces` of `body` by `angle` radians about the polyline
/// parting line through `polyline` (points on a plane perpendicular to
/// `across`), swept along `across`; `pull` points to the side whose faces
/// lean in going along it. Without `two_sided`, only the pull side is
/// drafted (the other side keeps its planes).
pub fn draft_faces_parting(
    body: &Body,
    faces: &[usize],
    polyline: &[[f64; 3]],
    across: [f64; 3],
    pull: [f64; 3],
    angle: f64,
    two_sided: bool,
) -> Result<Body> {
    if !(angle.is_finite() && angle.abs() < 89f64.to_radians()) {
        return Err(invalid(
            "draft: the angle must be finite and below 89 degrees",
        ));
    }
    let v = |a: [f64; 3]| Vector3::new(a[0], a[1], a[2]);
    let (s, p) = (v(across), v(pull));
    if s.magnitude() < 1e-12 || p.magnitude() < 1e-12 {
        return Err(invalid("draft: zero pull or parting sweep direction"));
    }
    let (s, p) = (s.normalize(), p.normalize());
    if s.dot(p).abs() > 1e-9 {
        return Err(invalid(
            "draft: the parting line's sketch plane must contain the pull direction",
        ));
    }
    if polyline.len() < 2 {
        return Err(invalid("draft: the parting line needs at least two points"));
    }
    let mut sh = shell_of(body)?;
    let tol = geo_tol(&sh);
    let h = p.cross(s);
    let p0 = Point3::new(polyline[0][0], polyline[0][1], polyline[0][2]);
    let mut xz: Vec<[f64; 2]> = Vec::new();
    for q in polyline {
        let d = Point3::new(q[0], q[1], q[2]) - p0;
        if d.dot(s).abs() > tol {
            return Err(invalid(
                "draft: the parting line's points are not on one plane across the sweep",
            ));
        }
        xz.push([d.dot(h), d.dot(p)]);
    }
    if xz.windows(2).all(|w| w[1][0] < w[0][0]) {
        xz.reverse();
    }
    if !xz.windows(2).all(|w| w[1][0] - w[0][0] > tol::MIN_LENGTH) {
        return Err(invalid(
            "draft: the parting line must run across the pull without turning back or \
             running along it (one height at every point)",
        ));
    }
    let pt = Parting { p0, h, p, s, xz };
    let sel: BTreeSet<usize> = faces.iter().copied().collect();
    if sel.is_empty() {
        return Err(invalid("draft: no faces"));
    }
    for &f in &sel {
        let face = sh
            .faces
            .get(f)
            .ok_or_else(|| invalid(format!("draft: no face {f}")))?;
        if plane_of(face).is_none() || face.boundaries.len() != 1 {
            return Err(invalid(format!(
                "draft: face {f} must be planar with one boundary loop"
            )));
        }
    }
    let old_shell = sh.clone();
    // Split the selected faces' edges where they cross the surface.
    let mut crossing_edges = BTreeSet::new();
    for &f in &sel {
        for ei in &sh.faces[f].boundaries[0] {
            crossing_edges.insert(ei.index);
        }
    }
    for e in crossing_edges {
        let (a, b) = sh.edges[e].vertices;
        let (ga, gb) = (pt.g(sh.vertices[a], tol)?, pt.g(sh.vertices[b], tol)?);
        if (ga > tol && gb < -tol) || (ga < -tol && gb > tol) {
            if !matches!(sh.edges[e].curve, Curve::Line(_)) {
                return Err(invalid(format!(
                    "draft: edge {e} crossing the parting line is not straight"
                )));
            }
            let x = pt.crossing(sh.vertices[a], sh.vertices[b], tol)?;
            split_edge(&mut sh, e, &[x]);
        }
    }
    // Split each selected face along the parting chain; record its pieces.
    // Piece: (face index, side +1 pull / -1 other, chain points in the
    // piece's loop order, chain edges in that order).
    let mut pieces: Vec<(usize, f64, Vec<usize>)> = Vec::new();
    let pull_dir = p;
    for &f in &sel {
        let wire = sh.faces[f].boundaries[0].clone();
        let gs: Vec<f64> = wire
            .iter()
            .map(|ei| pt.g(sh.vertices[start_of(&sh, ei)], tol))
            .collect::<Result<_>>()?;
        let any_pos = gs.iter().any(|g| *g > tol);
        let any_neg = gs.iter().any(|g| *g < -tol);
        if !(any_pos && any_neg) {
            return Err(invalid(format!(
                "draft: face {f} does not cross the parting line; with a broken parting line \
                 every face to draft must cross it"
            )));
        }
        let on: Vec<usize> = (0..wire.len()).filter(|&k| gs[k].abs() <= tol).collect();
        let [k1, k2] = on[..] else {
            return Err(invalid(format!(
                "draft: face {f} meets the parting line at {} boundary points; it must cross \
                 it once, entering and leaving",
                on.len()
            )));
        };
        let (o, n) = plane_of(&sh.faces[f]).ok_or_else(|| invalid("draft: face"))?;
        let (v1, v2) = (start_of(&sh, &wire[k1]), start_of(&sh, &wire[k2]));
        let (x1, x2) = (pt.x(sh.vertices[v1]), pt.x(sh.vertices[v2]));
        // Corners of the parting line strictly between, where their sweep
        // lines pierce the face.
        let fr = frame_on(o, n);
        let poly: Vec<[f64; 2]> = wire
            .iter()
            .map(|ei| to2(&fr, sh.vertices[start_of(&sh, ei)]))
            .collect();
        let mut corners = Vec::new();
        for (k, &[xk, zk]) in pt.xz.iter().enumerate() {
            if k == 0 || k + 1 == pt.xz.len() {
                continue;
            }
            if xk <= x1.min(x2) + tol || xk >= x1.max(x2) - tol {
                if (xk - x1).abs() <= tol || (xk - x2).abs() <= tol {
                    return Err(invalid(format!(
                        "draft: a corner of the parting line lies on an edge of face {f}"
                    )));
                }
                continue;
            }
            let pk = pt.p0 + pt.h * xk + pt.p * zk;
            if n.dot(pt.s).abs() < 1e-9 {
                return Err(invalid(format!(
                    "draft: face {f} runs along the parting line's sweep across a corner; its \
                     intersection with the parting surface is not one chain"
                )));
            }
            let lam = (o - pk).dot(n) / n.dot(pt.s);
            let c = pk + pt.s * lam;
            if !point_in(&poly, to2(&fr, c), tol) {
                return Err(invalid(format!(
                    "draft: the parting line leaves face {f} between its crossings"
                )));
            }
            corners.push((xk, c));
        }
        // Chain from v1 to v2.
        if x2 < x1 {
            corners.reverse();
        }
        let mut chain = vec![v1];
        for (_, c) in &corners {
            sh.vertices.push(*c);
            chain.push(sh.vertices.len() - 1);
        }
        chain.push(v2);
        let cedges: Vec<usize> = chain
            .windows(2)
            .map(|w| line_edge(&mut sh, w[0], w[1]))
            .collect();
        // Loop A: wire k1..k2 (v1 -> v2), then the chain back v2 -> v1.
        let mut la: Vec<Ix> = (k1..k2).map(|k| wire[k]).collect();
        la.extend(cedges.iter().rev().map(|&index| Ix {
            index,
            orientation: false,
        }));
        let mut lb: Vec<Ix> = (k2..wire.len()).chain(0..k1).map(|k| wire[k]).collect();
        lb.extend(cedges.iter().map(|&index| Ix {
            index,
            orientation: true,
        }));
        let side_a = if (k1..k2).any(|k| gs[k] > tol) {
            1.0
        } else {
            -1.0
        };
        let mut twin = sh.faces[f].clone();
        sh.faces[f].boundaries = vec![la];
        twin.boundaries = vec![lb];
        sh.faces.push(twin);
        let g = sh.faces.len() - 1;
        let mut rev = chain.clone();
        rev.reverse();
        pieces.push((f, side_a, rev));
        pieces.push((g, -side_a, chain));
    }
    // Facets: new planes, subdividing pieces with corners.
    let mut new_plane: BTreeMap<usize, (Point3, Vector3)> = BTreeMap::new();
    let drafted: BTreeSet<usize> = pieces
        .iter()
        .filter(|x| two_sided || x.1 > 0.0)
        .map(|x| x.0)
        .collect();
    for (face, side, chain) in pieces {
        if !drafted.contains(&face) {
            continue;
        }
        let dir = pull_dir * side;
        let n = plane_of(&sh.faces[face])
            .ok_or_else(|| invalid("draft: face"))?
            .1;
        let planes: Vec<(Point3, Vector3)> = chain
            .windows(2)
            .map(|w| {
                let (a, b) = (sh.vertices[w[0]], sh.vertices[w[1]]);
                turned(a, b - a, n, dir, angle)
            })
            .collect::<Result<_>>()?;
        if planes.len() == 1 {
            new_plane.insert(face, planes[0]);
            continue;
        }
        subdivide(&mut sh, face, &chain, &planes, &drafted, &mut new_plane)?;
    }
    finish(sh, &old_shell, &new_plane, tol)
}

/// Split drafted piece `face` (its loop: the parting `chain`, then the rest)
/// into one facet per chain segment with the planes `planes`, the facets
/// meeting in edges from the chain's corners to new vertices on one far
/// edge whose neighbour is not drafted.
fn subdivide(
    sh: &mut CS,
    face: usize,
    chain: &[usize],
    planes: &[(Point3, Vector3)],
    drafted: &BTreeSet<usize>,
    new_plane: &mut BTreeMap<usize, (Point3, Vector3)>,
) -> Result<()> {
    let wire = sh.faces[face].boundaries[0].clone();
    let m = wire.len();
    // Rotate the loop to start at the chain's first edge.
    let k0 = (0..m)
        .find(|&k| start_of(sh, &wire[k]) == chain[0] && end_of(sh, &wire[k]) == chain[1])
        .ok_or_else(|| invalid("draft: internal: the parting chain is not in the piece's loop"))?;
    let lp: Vec<Ix> = (0..m).map(|k| wire[(k0 + k) % m]).collect();
    let nseg = chain.len() - 1;
    let rest = &lp[nseg..];
    let ef = edge_faces(sh);
    let (o, n) = plane_of(&sh.faces[face]).ok_or_else(|| invalid("draft: face"))?;
    let fr = frame_on(o, n);
    // Rulings: from each corner along the facets' common line, projected on
    // the face, to the first edge of the rest of the loop.
    let mut hits = Vec::new();
    for i in 1..nseg {
        let c = sh.vertices[chain[i]];
        let r = planes[i - 1].1.cross(planes[i].1);
        // Into the piece: away from the chain, towards the loop's rest.
        let r2 = to2(&fr, c + r);
        let c2 = to2(&fr, c);
        let mut d = [r2[0] - c2[0], r2[1] - c2[1]];
        let inward = {
            // The piece lies to the left of its loop (counter-clockwise in
            // the face's outward frame): left of the chain's direction.
            let (a, b) = (
                to2(&fr, sh.vertices[chain[i - 1]]),
                to2(&fr, sh.vertices[chain[i + 1]]),
            );
            let t = [b[0] - a[0], b[1] - a[1]];
            -t[1] * d[0] + t[0] * d[1]
        };
        if inward.abs() < 1e-12 {
            return Err(invalid(
                "draft: facets at a parting corner meet along the parting line",
            ));
        }
        if inward < 0.0 {
            d = [-d[0], -d[1]];
        }
        let mut best: Option<(f64, usize, f64)> = None;
        for (j, ei) in rest.iter().enumerate() {
            let (a, b) = (
                to2(&fr, sh.vertices[start_of(sh, ei)]),
                to2(&fr, sh.vertices[end_of(sh, ei)]),
            );
            let e = [b[0] - a[0], b[1] - a[1]];
            let den = d[0] * e[1] - d[1] * e[0];
            if den.abs() < 1e-15 {
                continue;
            }
            let w = [a[0] - c2[0], a[1] - c2[1]];
            let lam = (w[0] * e[1] - w[1] * e[0]) / den;
            let u = (w[0] * d[1] - w[1] * d[0]) / den;
            if lam > 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&u) && best.is_none_or(|x| lam < x.0) {
                best = Some((lam, j, u));
            }
        }
        let Some((_, j, u)) = best else {
            return Err(invalid(
                "draft: a facet edge from a parting corner leaves the face",
            ));
        };
        if !(1e-6..=1.0 - 1e-6).contains(&u) {
            return Err(invalid(
                "draft: a facet edge from a parting corner reaches a corner of the face",
            ));
        }
        hits.push((j, u));
    }
    let j = hits[0].0;
    if hits.iter().any(|x| x.0 != j) {
        return Err(invalid(
            "draft: the facet edges from the parting corners reach different edges of the face; \
             they must all end on its far edge",
        ));
    }
    let far = rest[j];
    let nb: Vec<usize> = ef[far.index]
        .iter()
        .copied()
        .filter(|g| *g != face)
        .collect();
    if nb.len() != 1 || drafted.contains(&nb[0]) || plane_of(&sh.faces[nb[0]]).is_none() {
        return Err(invalid(
            "draft: the far edge the facets end on must border one planar face that is not drafted",
        ));
    }
    // Along the far edge in loop order the corners come last-first; order
    // by parameter along the edge's own direction.
    let mut us: Vec<(f64, usize)> = hits
        .iter()
        .enumerate()
        .map(|(i, h)| (if far.orientation { h.1 } else { 1.0 - h.1 }, i + 1))
        .collect();
    us.sort_by(|a, b| a.0.total_cmp(&b.0));
    if far.orientation {
        // Loop order equals edge order: corners must be descending.
        if us.windows(2).any(|w| w[1].1 > w[0].1) {
            return Err(invalid("draft: facet edges from the parting corners cross"));
        }
    } else if us.windows(2).any(|w| w[1].1 < w[0].1) {
        return Err(invalid("draft: facet edges from the parting corners cross"));
    }
    let (fa, fb) = sh.edges[far.index].vertices;
    let (pa, pb) = (sh.vertices[fa], sh.vertices[fb]);
    let pts: Vec<Point3> = us.iter().map(|(u, _)| pa + (pb - pa) * *u).collect();
    let newv = split_edge(sh, far.index, &pts);
    // Corner i -> its new far vertex.
    let mut tv = BTreeMap::new();
    for (k, (_, i)) in us.iter().enumerate() {
        tv.insert(*i, newv[k]);
    }
    let rulings: BTreeMap<usize, usize> = (1..nseg)
        .map(|i| (i, line_edge(sh, chain[i], tv[&i])))
        .collect();
    // Rebuild: walk the piece's loop (now with the far edge in pieces) from
    // the chain start; facet i collects segment i, then the loop's rest
    // between ruling i + 1 and ruling i.
    let wire = sh.faces[face].boundaries[0].clone();
    let m = wire.len();
    let k0 = (0..m)
        .find(|&k| start_of(sh, &wire[k]) == chain[0] && end_of(sh, &wire[k]) == chain[1])
        .ok_or_else(|| invalid("draft: internal: lost the parting chain"))?;
    let lp: Vec<Ix> = (0..m).map(|k| wire[(k0 + k) % m]).collect();
    let rest: Vec<Ix> = lp[nseg..].to_vec();
    // Split the rest at the new far vertices: rest runs from chain end
    // (facet nseg-1) to chain start (facet 0).
    let mut parts: Vec<Vec<Ix>> = vec![Vec::new()];
    let by_vertex: BTreeMap<usize, usize> = tv.iter().map(|(i, v)| (*v, *i)).collect();
    let mut owners = vec![nseg - 1];
    for ei in &rest {
        parts.last_mut().expect("parts").push(*ei);
        if let Some(&i) = by_vertex.get(&end_of(sh, ei)) {
            parts.push(Vec::new());
            owners.push(i - 1);
        }
    }
    if owners.len() != nseg {
        return Err(invalid("draft: internal: facet split mismatch"));
    }
    let template = sh.faces[face].clone();
    for (part, &i) in parts.into_iter().zip(&owners) {
        let mut lw = vec![lp[i]];
        if i + 1 < nseg {
            lw.push(Ix {
                index: rulings[&(i + 1)],
                orientation: true,
            });
        }
        lw.extend(part);
        if i > 0 {
            lw.push(Ix {
                index: rulings[&i],
                orientation: false,
            });
        }
        let g = if i == 0 {
            face
        } else {
            sh.faces.push(template.clone());
            sh.faces.len() - 1
        };
        sh.faces[g].boundaries = vec![lw];
        new_plane.insert(g, planes[i]);
    }
    Ok(())
}

/// Point strictly inside a simple polygon (and farther than `tol` from it).
fn point_in(poly: &[[f64; 2]], q: [f64; 2], tol: f64) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if (q[0] - a[0] - t * dx).hypot(q[1] - a[1] - t * dy) <= tol {
            return false;
        }
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) * dx / dy {
            inside = !inside;
        }
    }
    inside
}
