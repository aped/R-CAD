//! Exact concave root fillet where a round boss (cylinder, or a cone
//! narrowing away from the base by a draft) meets the face it stands on, built directly into the topology like [`super::countersink`] (a
//! boolean would meet the fillet's tangent contacts with the face and the
//! boss, which Truck cannot resolve).
//!
//! Precondition: a boss wall of coaxial cylinder faces of radius `r`
//! (material inside) whose base loop is a whole inner loop of a planar face
//! perpendicular to the axis (the base), the wall reaching higher than the
//! fillet radius `f`, and nothing else on the base within `r + f` of the
//! axis. Construction, with the base's outward normal `n`:
//!
//! * every base-loop vertex `v` moves to `v + f n` (it stays on the
//!   cylinder) and the loop arcs are translated by `f n` (they stay the
//!   wall's bottom edges, exact circles of radius `r`);
//! * new vertices `c + (v − c)(r + f)/r` on the base and the loop arcs
//!   scaled radially by `(r + f)/r` (affine, exact) replace the base's
//!   inner loop;
//! * one toroidal face per loop arc: the tensor product of the arc's
//!   rational NURBS with the rational quarter circle `(r, f) → (r, 0) →
//!   (r + f, 0)` (weights 1, √½, 1) in (radius, height), rescaled about the
//!   axis, which is exactly the arc revolved section (affine images of one
//!   rational arc), bounded by two quarter-circle edges at the loop's
//!   vertices.
//!
//! Checked like every direct edit (edges on surfaces, one closed shell) plus
//! point probes inside the fillet and in the air beyond it.
use super::*;

/// Round the base edge of the round boss of radius `boss_diameter / 2`
/// whose axis passes through `axis_point` (on the base face's plane) along
/// `axis` (either sense) with a concave fillet of radius `radius`.
pub fn root_round(
    body: &Body,
    axis_point: [f64; 3],
    axis: [f64; 3],
    boss_diameter: f64,
    radius: f64,
    half_angle: f64,
) -> Result<Body> {
    if !(half_angle.is_finite() && (0.0..std::f64::consts::FRAC_PI_4).contains(&half_angle)) {
        return Err(invalid(
            "root fillet: the wall's draft must be within 0..45 degrees",
        ));
    }
    let finite = [boss_diameter, radius]
        .iter()
        .chain(&axis_point)
        .chain(&axis)
        .all(|v| v.is_finite());
    if !finite {
        return Err(invalid("root fillet: non-finite input"));
    }
    let (r, f) = (boss_diameter / 2.0, radius);
    if f < tol::MIN_LENGTH || r < tol::MIN_LENGTH {
        return Err(invalid(format!(
            "root fillet: radius {f} mm and boss diameter {boss_diameter} mm must be positive"
        )));
    }
    let a = Vector3::new(axis[0], axis[1], axis[2]);
    if a.magnitude() < 1e-12 {
        return Err(invalid("root fillet: zero axis"));
    }
    let a = a.normalize();
    let c0 = Point3::new(axis_point[0], axis_point[1], axis_point[2]);
    let mut sh = shell_of(body)?;
    let tol = geo_tol(&sh);
    // Boss wall: the non-planar faces on the cone (a cylinder when
    // `half_angle` is 0) of radius r at the base plane through the axis
    // point, narrowing by tan(half_angle) per mm along the base's normal;
    // material inside it.
    let tan_a = half_angle.tan();
    let mut cyl = BTreeSet::new();
    for (i, face) in sh.faces.iter().enumerate() {
        if matches!(face.surface, Surface::Plane(_)) {
            continue;
        }
        let Ok(samples) = face_samples(&sh, face) else {
            continue;
        };
        if samples.len() < 3 {
            continue;
        }
        let on = samples.iter().all(|(p, nrm)| {
            let d = *p - c0;
            let h = d.dot(a);
            let radial = d - a * h;
            let rho = radial.magnitude();
            // Both axis senses: the base normal is found below; accept a
            // wall narrowing either way here and check the sense after.
            let fit = (rho - (r - h * tan_a)).abs() <= tol || (rho - (r + h * tan_a)).abs() <= tol;
            fit && rho > tol && nrm.dot(radial / rho) > 0.0
        });
        if on {
            cyl.insert(i);
        }
    }
    if cyl.is_empty() {
        return Err(invalid(format!(
            "root fillet: no Ø{boss_diameter} mm boss wall on this axis"
        )));
    }
    let cyl_edges: BTreeSet<usize> = cyl.iter().flat_map(|g| face_edges(&sh.faces[*g])).collect();
    // Base: a plane perpendicular to the axis through the axis point with a
    // whole inner loop of boss-wall edges.
    let mut base = None;
    for (i, face) in sh.faces.iter().enumerate() {
        let Some((o, n)) = plane_of(face) else {
            continue;
        };
        if n.cross(a).magnitude() > 1e-9 || (o - c0).dot(n).abs() > tol {
            continue;
        }
        for (w, wire) in face.boundaries.iter().enumerate() {
            if !wire.is_empty() && wire.iter().all(|e| cyl_edges.contains(&e.index)) {
                if face.boundaries.len() < 2 {
                    return Err(invalid(
                        "root fillet: the base face is bounded only by the boss",
                    ));
                }
                if base.is_some() {
                    return Err(invalid("root fillet: the boss has two base loops here"));
                }
                base = Some((i, w, n));
            }
        }
    }
    let Some((bf, wire_idx, n)) = base else {
        return Err(invalid(
            "root fillet: the boss does not stand on a planar face perpendicular to its axis at \
             the given point",
        ));
    };
    let base_origin = plane_of(&sh.faces[bf]).map_or(c0, |x| x.0);
    let centre = c0 - n * (c0 - base_origin).dot(n);
    if half_angle > 0.0 {
        for &g in &cyl {
            let ok = face_samples(&sh, &sh.faces[g])?.iter().all(|(p, _)| {
                let d = *p - centre;
                let h = d.dot(n);
                ((d - n * h).magnitude() - (r - h * tan_a)).abs() <= tol
            });
            if !ok {
                return Err(invalid(
                    "root fillet: the boss wall does not narrow away from the base by the given draft",
                ));
            }
        }
    }
    // Fillet circle tangent to the base and to the wall's generator: centre
    // at height f, radius `cx` from the axis; tangent points W on the wall,
    // B on the base.
    let (sa, ca) = half_angle.sin_cos();
    let cx = r + f * (1.0 - sa) / ca;
    let h_t = f * (1.0 - sa);
    let r_t = cx - f * ca;
    let big = cx;
    // Clearance on the base face.
    {
        let fr = frame_on(base_origin, n);
        let c2 = to2(&fr, centre);
        for (w, wire) in sh.faces[bf].boundaries.iter().enumerate() {
            if w == wire_idx {
                continue;
            }
            for ei in wire {
                let Some(e2) = edge2(&sh, ei.index, ei.orientation, &fr, tol)? else {
                    return Err(invalid(
                        "root fillet: the base face has a free-form edge; clearance cannot be \
                         checked exactly",
                    ));
                };
                let d = dist2(c2, &e2);
                if d <= big + tol::MIN_LENGTH {
                    return Err(invalid(format!(
                        "root fillet R{f} mm does not fit on the base face: another edge is \
                         {d:.6} mm from the boss axis"
                    )));
                }
            }
        }
    }
    let lp: Vec<(usize, bool)> = sh.faces[bf].boundaries[wire_idx]
        .iter()
        .map(|e| (e.index, e.orientation))
        .collect();
    let lset: BTreeSet<usize> = lp.iter().map(|x| x.0).collect();
    let lverts = edge_vertices(&sh, &lset);
    let vf = vertex_faces(&sh);
    for v in &lverts {
        if vf[*v].iter().any(|g| *g != bf && !cyl.contains(g)) {
            return Err(invalid(
                "root fillet: other faces meet the boss's base loop (a feature already there)",
            ));
        }
    }
    // Wall edges along the axis from the loop: shortened by f at the bottom.
    let mut seams = BTreeSet::new();
    for &e in &cyl_edges {
        if lset.contains(&e) {
            continue;
        }
        let (p, q) = sh.edges[e].vertices;
        let (on_p, on_q) = (lverts.contains(&p), lverts.contains(&q));
        if !(on_p || on_q) {
            continue;
        }
        if on_p && on_q {
            return Err(invalid(
                "root fillet: a boss-wall edge joins two base-loop vertices",
            ));
        }
        let (fixed, moving) = if on_p { (q, p) } else { (p, q) };
        let (f0, m0) = (sh.vertices[fixed], sh.vertices[moving]);
        let up = (f0 - m0).dot(n);
        let g = (f0 - m0).normalize();
        let pts = curve_samples(&sh.edges[e].curve, 16)?;
        if pts
            .iter()
            .any(|x| ((*x - m0) - g * (*x - m0).dot(g)).magnitude() > tol)
        {
            return Err(invalid(
                "root fillet: the boss wall has an edge that is not a straight generator",
            ));
        }
        if up - h_t < tol::MIN_LENGTH {
            return Err(invalid(format!(
                "root fillet R{f} mm reaches the top of the boss wall ({up:.6} mm high)"
            )));
        }
        seams.insert(e);
    }
    let lift = Matrix4::from_translation(n * h_t) * radial_scaling(centre, a, r_t / r);
    let scale_out = radial_scaling(centre, a, big / r);
    // Arc weight: cos of half its sweep (90° − the wall's draft).
    let w45 = ((std::f64::consts::FRAC_PI_2 - half_angle) / 2.0).cos();
    // New outer vertices on the base; lift the loop vertices. Keep the
    // original positions (the quarter circles' corners).
    let mut out_v = BTreeMap::new();
    let mut corner = BTreeMap::new();
    for &v in &lverts {
        let p = sh.vertices[v];
        corner.insert(v, p);
        out_v.insert(v, sh.vertices.len());
        sh.vertices.push(scale_out.transform_point(p));
        sh.vertices[v] = lift.transform_point(p);
    }
    for &e in &seams {
        let (p, q) = sh.edges[e].vertices;
        sh.edges[e].curve = Curve::Line(Line(sh.vertices[p], sh.vertices[q]));
    }
    let nurbs = |c: &Curve| -> Result<NurbsCurve<Vector4>> {
        match c {
            Curve::NurbsCurve(c) => Ok(c.clone()),
            Curve::BSplineCurve(c) => Ok(NurbsCurve::from(c.clone())),
            _ => Err(invalid(
                "root fillet: the boss's base edge is not a (rational) B-spline arc",
            )),
        }
    };
    let h4 = |p: Point3, w: f64| Vector4::new(p.x * w, p.y * w, p.z * w, w);
    // Quarter-circle edges at the loop vertices: lifted vertex -> corner ->
    // outer vertex.
    let mut side = BTreeMap::new();
    for &v in &lverts {
        side.insert(v, sh.edges.len());
        let arc = BSplineCurve::new(
            KnotVec::bezier_knot(2),
            vec![
                h4(sh.vertices[v], 1.0),
                h4(corner[&v], w45),
                h4(sh.vertices[out_v[&v]], 1.0),
            ],
        );
        sh.edges.push(truck_topology::compress::CompressedEdge {
            vertices: (v, out_v[&v]),
            curve: Curve::NurbsCurve(NurbsCurve::new(arc)),
        });
    }
    let base_flip = !sh.faces[bf].orientation;
    let mut touched: BTreeSet<usize> = seams.clone();
    touched.extend(side.values().copied());
    // Profile in (radius, height, weight): wall end, corner, base end.
    let profile = [(r_t, h_t, 1.0), (r, 0.0, w45), (big, 0.0, 1.0)];
    for (k, &(e, o)) in lp.iter().enumerate() {
        let original = sh.edges[e].curve.clone();
        let src = nurbs(&original)?;
        let mut lower = original.clone();
        lower.transform_by(lift);
        let mut upper = original;
        upper.transform_by(scale_out);
        let (va, vb) = sh.edges[e].vertices;
        sh.edges[e].curve = lower;
        let eu = sh.edges.len();
        sh.edges.push(truck_topology::compress::CompressedEdge {
            vertices: (out_v[&va], out_v[&vb]),
            curve: upper,
        });
        sh.faces[bf].boundaries[wire_idx][k].index = eu;
        touched.extend([e, eu]);
        let o = o != base_flip;
        let (s, t) = if o { (va, vb) } else { (vb, va) };
        use truck_topology::compress::CompressedEdgeIndex as Ix;
        let wire = vec![
            Ix {
                index: e,
                orientation: o,
            },
            Ix {
                index: side[&t],
                orientation: true,
            },
            Ix {
                index: eu,
                orientation: !o,
            },
            Ix {
                index: side[&s],
                orientation: false,
            },
        ];
        // Tensor product: rows along the arc (u), columns along the profile.
        let knots_u = src.knot_vec().clone();
        let (u0, u1) = src.range_tuple();
        let rows: Vec<Vec<Vector4>> = src
            .control_points()
            .iter()
            .map(|hp| {
                let w = hp.w;
                let p = Point3::new(hp.x / w, hp.y / w, hp.z / w);
                let radial = (p - centre) - a * (p - centre).dot(a);
                profile
                    .iter()
                    .map(|&(rho, z, wv)| h4(centre + radial * (rho / r) + n * z, w * wv))
                    .collect()
            })
            .collect();
        let surf = BSplineSurface::new((knots_u, KnotVec::bezier_knot(2)), rows);
        let surface = Surface::NurbsSurface(NurbsSurface::new(surf));
        // Outward = away from the fillet's material: towards the circle of
        // centres at radius r + f, height f.
        let pm = surface.subs(0.5 * (u0 + u1), 0.5);
        let nm = surface.normal(0.5 * (u0 + u1), 0.5);
        let radial = {
            let d = pm - centre;
            (d - a * d.dot(a)).normalize()
        };
        let ctr = centre + n * f + radial * big;
        let orientation = nm.dot(ctr - pm) > 0.0;
        let wire = if orientation {
            wire
        } else {
            wire.into_iter()
                .rev()
                .map(|x| Ix {
                    index: x.index,
                    orientation: !x.orientation,
                })
                .collect()
        };
        sh.faces.push(CompressedFace {
            boundaries: vec![wire],
            orientation,
            surface,
        });
    }
    check_edges_on_faces(&sh, &touched, tol)?;
    let out = rebuild(sh)?;
    check_solid(&out)?;
    let u = {
        let x = crate::step_in::least_aligned_axis(a);
        x - a * x.dot(a)
    }
    .normalize();
    let material = centre + u * (r + 0.1 * f) + n * (0.05 * f);
    let air = centre + u * (cx - 0.2 * f) + n * (0.8 * f);
    let m = crate::point_membership(&out, &[p3(material), p3(air)])?;
    if m[0] != Some(true) || m[1] != Some(false) {
        return Err(KernelError::InvalidResult(format!(
            "root fillet failed the probe check (material {:?}, air {:?})",
            m[0], m[1]
        )));
    }
    Ok(out)
}
