//! Exact draft of planar faces of a polyhedral region, one- or two-sided
//! about a parting plane (fixed-plane and parting-line drafts of planar
//! walls).
//!
//! Construction, without booleans:
//! * two-sided: every selected face that crosses the parting plane is split
//!   along it (new vertices on the edges crossing the plane, shared edges
//!   split once; a new straight edge across the face);
//! * every selected face (or half) turns about its line on the parting plane
//!   by the draft angle, leaning in as it goes away from the plane: along the
//!   pull on the pull side, against it on the other side (one-sided: every
//!   face along the pull);
//! * every vertex of a changed face becomes the intersection of the planes
//!   of all faces meeting there (least squares, refused unless they meet in
//!   one point within tolerance), and the edges at those vertices become the
//!   straight segments between them.
//!
//! Preconditions (refused by name): the selected faces are planar with one
//! boundary loop, every face touching a moved vertex is planar, every edge
//! at a moved vertex is straight. Checks: every touched loop stays simple
//! with its orientation and area, edges on their faces, one closed shell.
use super::*;

/// Draft faces `faces` (indices in face order) of `body` by `angle`
/// radians about the plane through `plane_point` with unit normal
/// `plane_normal`; `pull` is the pull direction (faces lean in going along
/// it on its side of the plane). With `two_sided`, faces crossing the plane
/// are split and the far side leans in going the other way.
pub fn draft_faces(
    body: &Body,
    faces: &[usize],
    plane_point: [f64; 3],
    plane_normal: [f64; 3],
    pull: [f64; 3],
    angle: f64,
    two_sided: bool,
) -> Result<Body> {
    if !(angle.is_finite() && angle.abs() < 89f64.to_radians()) {
        return Err(invalid(
            "draft: the angle must be finite and below 89 degrees",
        ));
    }
    let po = Point3::new(plane_point[0], plane_point[1], plane_point[2]);
    let pn = Vector3::new(plane_normal[0], plane_normal[1], plane_normal[2]);
    let pu = Vector3::new(pull[0], pull[1], pull[2]);
    if pn.magnitude() < 1e-12 || pu.magnitude() < 1e-12 {
        return Err(invalid("draft: zero plane normal or pull direction"));
    }
    let (pn, pu) = (pn.normalize(), pu.normalize());
    let mut sh = shell_of(body)?;
    let tol = geo_tol(&sh);
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
    let dist = |p: Point3| (p - po).dot(pn);
    // Pull side of the plane: +1 if the pull leaves the plane along pn.
    let pull_side = if pu.dot(pn) >= 0.0 { 1.0 } else { -1.0 };
    let old_shell = sh.clone();
    // Face -> which side it drafts for (+1 pull side, -1 far side).
    let mut side_of: BTreeMap<usize, f64> = BTreeMap::new();
    if two_sided {
        // Split crossing edges of selected faces once.
        let mut split_at: BTreeMap<usize, usize> = BTreeMap::new();
        for &f in &sel {
            for ei in sh.faces[f].boundaries[0].clone() {
                let e = ei.index;
                if split_at.contains_key(&e) {
                    continue;
                }
                let (a, b) = sh.edges[e].vertices;
                let (da, db) = (dist(sh.vertices[a]), dist(sh.vertices[b]));
                if (da > tol && db < -tol) || (da < -tol && db > tol) {
                    if !matches!(sh.edges[e].curve, Curve::Line(_)) {
                        return Err(invalid(format!(
                            "draft: edge {e} crossing the plane is not straight"
                        )));
                    }
                    let t = da / (da - db);
                    let x = sh.vertices[a] + (sh.vertices[b] - sh.vertices[a]) * t;
                    let v = sh.vertices.len();
                    sh.vertices.push(x);
                    let e2 = sh.edges.len();
                    sh.edges.push(truck_topology::compress::CompressedEdge {
                        vertices: (v, b),
                        curve: Curve::Line(Line(x, sh.vertices[b])),
                    });
                    sh.edges[e].vertices = (a, v);
                    sh.edges[e].curve = Curve::Line(Line(sh.vertices[a], x));
                    split_at.insert(e, e2);
                }
            }
        }
        // Every wire using a split edge takes both halves.
        use truck_topology::compress::CompressedEdgeIndex as Ix;
        for face in sh.faces.iter_mut() {
            for wire in face.boundaries.iter_mut() {
                let mut out = Vec::with_capacity(wire.len() + 2);
                for ei in wire.iter() {
                    match split_at.get(&ei.index) {
                        Some(&e2) if ei.orientation => {
                            out.push(*ei);
                            out.push(Ix {
                                index: e2,
                                orientation: true,
                            });
                        }
                        Some(&e2) => {
                            out.push(Ix {
                                index: e2,
                                orientation: false,
                            });
                            out.push(*ei);
                        }
                        None => out.push(*ei),
                    }
                }
                *wire = out;
            }
        }
        // Split each selected face along its two on-plane vertices.
        let start_of = |sh: &CS, ei: &Ix| {
            let (a, b) = sh.edges[ei.index].vertices;
            if ei.orientation { a } else { b }
        };
        for &f in &sel {
            let wire = sh.faces[f].boundaries[0].clone();
            let on: Vec<usize> = (0..wire.len())
                .filter(|&k| dist(sh.vertices[start_of(&sh, &wire[k])]).abs() <= tol)
                .collect();
            let side = |sh: &CS, k: usize| dist(sh.vertices[start_of(sh, &wire[k])]);
            let any_pos = (0..wire.len()).any(|k| side(&sh, k) > tol);
            let any_neg = (0..wire.len()).any(|k| side(&sh, k) < -tol);
            if !(any_pos && any_neg) {
                side_of.insert(f, if any_pos { 1.0 } else { -1.0 } * pull_side);
                continue;
            }
            let [k1, k2] = on.as_slice() else {
                return Err(invalid(format!(
                    "draft: face {f} meets the parting plane in {} points; a convex face is \
                     split at two",
                    on.len()
                )));
            };
            let (k1, k2) = (*k1, *k2);
            let (v1, v2) = (start_of(&sh, &wire[k1]), start_of(&sh, &wire[k2]));
            let en = sh.edges.len();
            sh.edges.push(truck_topology::compress::CompressedEdge {
                vertices: (v1, v2),
                curve: Curve::Line(Line(sh.vertices[v1], sh.vertices[v2])),
            });
            // Loop A: edges k1..k2 (v1 -> v2) then the new edge v2 -> v1.
            let mut la: Vec<Ix> = (k1..k2).map(|k| wire[k]).collect();
            la.push(Ix {
                index: en,
                orientation: false,
            });
            let mut lb: Vec<Ix> = (k2..wire.len()).chain(0..k1).map(|k| wire[k]).collect();
            lb.push(Ix {
                index: en,
                orientation: true,
            });
            let sa = la
                .iter()
                .map(|ei| dist(sh.vertices[start_of(&sh, ei)]))
                .fold(0.0, f64::max)
                + la.iter()
                    .map(|ei| dist(sh.vertices[start_of(&sh, ei)]))
                    .fold(0.0, f64::min);
            let mut twin = sh.faces[f].clone();
            sh.faces[f].boundaries = vec![la];
            twin.boundaries = vec![lb];
            let g = sh.faces.len();
            sh.faces.push(twin);
            let s = if sa > 0.0 { 1.0 } else { -1.0 };
            side_of.insert(f, s * pull_side);
            side_of.insert(g, -s * pull_side);
        }
    } else {
        for &f in &sel {
            side_of.insert(f, 1.0);
        }
    }
    // New planes: each face turned about its hinge line on the plane.
    let mut new_plane: BTreeMap<usize, (Point3, Vector3)> = BTreeMap::new();
    for (&f, &s) in &side_of {
        let (o, n) = plane_of(&sh.faces[f]).ok_or_else(|| invalid("draft: face"))?;
        let l = n.cross(pn);
        if l.magnitude() < 1e-9 {
            return Err(invalid(format!(
                "draft: face {f} is parallel to the parting plane"
            )));
        }
        let l = l.normalize();
        // A point on both planes.
        let (d1, d2) = (n.dot(o.to_vec()), pn.dot(po.to_vec()));
        let ll = n.cross(pn).magnitude2();
        let p0 = Point3::from_vec((pn.cross(n.cross(pn)) * d1 + n.cross(pn).cross(n) * d2) / ll);
        let dir = pu * s;
        let mut t = l.cross(n);
        if t.dot(dir) < 0.0 {
            t = -t;
        }
        let n1 = (n * angle.cos() + t * angle.sin()).normalize();
        new_plane.insert(f, (p0, n1));
    }
    finish(sh, &old_shell, &new_plane, tol)
}

/// Move every vertex of the faces in `new_plane` to the intersection of the
/// planes of the faces meeting there (the new planes where given), make the
/// edges at them straight, give those faces their new planes, and check
/// every touched loop (simple, same sense as in `old_shell`), the edges on
/// their faces and the closed shell.
pub(super) fn finish(
    mut sh: CS,
    old_shell: &CS,
    new_plane: &BTreeMap<usize, (Point3, Vector3)>,
    tol: f64,
) -> Result<Body> {
    // Faces of each vertex (after splitting).
    let vf = vertex_faces(&sh);
    let changed: BTreeSet<usize> = new_plane.keys().copied().collect();
    let mut moved = BTreeSet::new();
    for &f in &changed {
        moved.extend(face_edges(&sh.faces[f]).flat_map(|e| {
            let (a, b) = sh.edges[e].vertices;
            [a, b]
        }));
    }
    let plane_now = |g: usize, sh: &CS| -> Result<(Point3, Vector3)> {
        new_plane
            .get(&g)
            .copied()
            .or_else(|| plane_of(&sh.faces[g]))
            .ok_or_else(|| {
                invalid(format!(
                    "draft: face {g} next to a drafted face is not planar; drafts need planar \
                     neighbours"
                ))
            })
    };
    let mut pos = BTreeMap::new();
    for &v in &moved {
        let mut m = Matrix3::from_value(0.0);
        let mut rhs = Vector3::new(0.0, 0.0, 0.0);
        let planes: Vec<(Point3, Vector3)> = vf[v]
            .iter()
            .map(|&g| plane_now(g, &sh))
            .collect::<Result<_>>()?;
        for (o, n) in &planes {
            let d = n.dot(o.to_vec());
            m += Matrix3::from_cols(*n * n.x, *n * n.y, *n * n.z);
            rhs += *n * d;
        }
        if m.determinant().abs() < 1e-12 {
            return Err(invalid(
                "draft: the faces at a vertex do not meet in one point (fewer than three \
                 independent planes)",
            ));
        }
        let x = Point3::from_vec(
            m.invert()
                .ok_or_else(|| invalid("draft: singular vertex"))?
                * rhs,
        );
        for (o, n) in &planes {
            if (x - *o).dot(*n).abs() > tol {
                return Err(invalid(
                    "draft: the drafted faces do not meet in one point at a vertex (more than \
                     three faces with inconsistent planes)",
                ));
            }
        }
        pos.insert(v, x);
    }
    for (v, x) in &pos {
        sh.vertices[*v] = *x;
    }
    let mut touched = BTreeSet::new();
    for (e, ce) in sh.edges.iter_mut().enumerate() {
        let (a, b) = ce.vertices;
        if pos.contains_key(&a) || pos.contains_key(&b) {
            if !matches!(ce.curve, Curve::Line(_)) {
                return Err(invalid(format!(
                    "draft: edge {e} at a moved vertex is not straight"
                )));
            }
            ce.curve = Curve::Line(Line(sh.vertices[a], sh.vertices[b]));
            touched.insert(e);
        }
    }
    for (&f, &(o1, n1)) in new_plane {
        let s_old = match &sh.faces[f].surface {
            Surface::Plane(pl) => pl.normal(),
            _ => return Err(invalid("draft: face is not planar")),
        };
        let sn = if n1.dot(s_old) > 0.0 { n1 } else { -n1 };
        let fr = frame_on(o1, sn);
        let (ox, oy) = (
            Vector3::new(fr.x[0], fr.x[1], fr.x[2]),
            Vector3::new(fr.y[0], fr.y[1], fr.y[2]),
        );
        sh.faces[f].surface = Surface::Plane(Plane::new(o1, o1 + ox, o1 + oy));
    }
    // Every face touching a moved vertex keeps simple loops of the same sense.
    let mut faces_touched = BTreeSet::new();
    for v in pos.keys() {
        faces_touched.extend(vf[*v].iter().copied());
    }
    for &g in &faces_touched {
        let (og, ng) = plane_now(g, &sh)?;
        let frg = frame_on(og, ng);
        for wire in &sh.faces[g].boundaries {
            let poly = |s: &CS| -> Vec<[f64; 2]> {
                wire.iter()
                    .map(|ei| {
                        let (a, b) = s.edges[ei.index].vertices;
                        to2(&frg, s.vertices[if ei.orientation { a } else { b }])
                    })
                    .collect()
            };
            let area = |c: &[[f64; 2]]| {
                let n = c.len();
                (0..n)
                    .map(|i| c[i][0] * c[(i + 1) % n][1] - c[(i + 1) % n][0] * c[i][1])
                    .sum::<f64>()
                    / 2.0
            };
            let after = poly(&sh);
            if area(&after).abs() < tol::MIN_LENGTH * tol::MIN_LENGTH || !simple_loop(&after) {
                return Err(invalid(format!(
                    "draft: a loop of face {g} would degenerate or cross itself (the angle is too \
                     large for this face)"
                )));
            }
            // Same sense as before when the face existed before.
            if g < old_shell.faces.len() && !changed.contains(&g) {
                let before: Vec<[f64; 2]> = wire
                    .iter()
                    .filter(|ei| ei.index < old_shell.edges.len())
                    .map(|ei| {
                        let (a, b) = sh.edges[ei.index].vertices;
                        let v = if ei.orientation { a } else { b };
                        to2(
                            &frg,
                            old_shell.vertices.get(v).copied().unwrap_or(sh.vertices[v]),
                        )
                    })
                    .collect();
                if before.len() >= 3 && area(&before).signum() != area(&after).signum() {
                    return Err(invalid(format!("draft: face {g} would turn inside out")));
                }
            }
        }
    }
    check_edges_on_faces(&sh, &touched, tol)?;
    let out = rebuild(sh)?;
    check_solid(&out)?;
    Ok(out)
}

/// No two non-adjacent segments of the closed polygon cross.
pub(super) fn simple_loop(p: &[[f64; 2]]) -> bool {
    let n = p.len();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (a, b, c, d) = (p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]);
            let (d1, d2) = (cross(a, b, c), cross(a, b, d));
            let (d3, d4) = (cross(c, d, a), cross(c, d, b));
            if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                return false;
            }
        }
    }
    n >= 3
}
