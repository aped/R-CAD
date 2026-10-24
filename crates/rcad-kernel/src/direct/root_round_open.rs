//! Exact concave root fillet along an open run of a round (cylindrical)
//! wall's base edge, ending at the wall's planar end faces: the root of a
//! web or rib along a sketch arc, on either side of it. Built directly into
//! the topology like [`super::root_round`], whose construction it extends.
//!
//! Precondition: wall faces on one cylinder of radius `r` about the axis,
//! all with material on the same side (inside: the fillet runs outwards on
//! the base; outside: inwards, needing `r > f`), whose base edges are one
//! contiguous open run of a wire of a planar face perpendicular to the axis
//! (the base). At each end of the run exactly three faces meet: the base, a
//! wall face, and a planar end face containing the axis (radial), whose
//! base edge is a straight segment running away from the fillet side. The
//! wall reaches higher than `f`, and nothing else on the base lies within
//! the fillet's band. Construction, with the base's outward normal `n` and
//! `s = ±1` the side of the fillet (outwards / inwards):
//!
//! * the run's vertices move up by `f n` (they stay on the wall) and its
//!   arcs are translated by `f n`;
//! * new vertices `c + (v − c)(r + s f)/r` on the base and the run's arcs
//!   scaled radially by `(r + s f)/r` replace the run in the base's wire;
//! * one toroidal face per arc (the arc's rational NURBS times the quarter
//!   circle `(r, f) → (r, 0) → (r + s f, 0)` in (radius, height));
//! * at each end, the quarter circle lies in the radial end face, which
//!   gains it between its wall edge and its base edge; the base edge is
//!   extended to the new base vertex.
//!
//! Checked like every direct edit (edges on surfaces, one closed shell),
//! plus point probes and the added volume against Pappus' theorem.
use super::*;

pub(super) fn root_round_open(body: &Body, c0: Point3, a: Vector3, r: f64, f: f64) -> Result<Body> {
    let mut sh = shell_of(body)?;
    let tol = geo_tol(&sh);
    // Wall faces: non-planar faces on the cylinder, with the material side.
    let mut cyl = BTreeSet::new();
    let mut side = None;
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
        let mut sense = 0.0;
        let on = samples.iter().all(|(p, nrm)| {
            let d = *p - c0;
            let radial = d - a * d.dot(a);
            let rho = radial.magnitude();
            sense += nrm.dot(radial / rho.max(1e-300));
            (rho - r).abs() <= tol && rho > tol
        });
        if !on {
            continue;
        }
        let s = if sense > 0.0 { 1.0 } else { -1.0 };
        if side.is_some_and(|x| x != s) {
            return Err(invalid(
                "root fillet: the round wall has material on both sides on this axis",
            ));
        }
        side = Some(s);
        cyl.insert(i);
    }
    let Some(s) = side else {
        return Err(invalid(format!(
            "root fillet: no round wall of radius {r} mm on this axis"
        )));
    };
    let big = r + s * f;
    if big < tol::MIN_LENGTH {
        return Err(invalid(format!(
            "root fillet R{f} mm does not fit inside the wall's radius {r} mm"
        )));
    }
    let cyl_edges: BTreeSet<usize> = cyl.iter().flat_map(|g| face_edges(&sh.faces[*g])).collect();
    // Base: a plane perpendicular to the axis through the axis point with
    // one contiguous open run of wall edges in one wire.
    let mut base = None;
    for (i, face) in sh.faces.iter().enumerate() {
        let Some((o, n)) = plane_of(face) else {
            continue;
        };
        if n.cross(a).magnitude() > 1e-9 || (o - c0).dot(n).abs() > tol {
            continue;
        }
        for (w, wire) in face.boundaries.iter().enumerate() {
            let on: Vec<bool> = wire.iter().map(|e| cyl_edges.contains(&e.index)).collect();
            let len = on.iter().filter(|x| **x).count();
            if len == 0 {
                continue;
            }
            if len == on.len() {
                return Err(invalid(
                    "root fillet: the wall's base is a whole loop (a boss), not an open run",
                ));
            }
            let starts: Vec<usize> = (0..on.len())
                .filter(|&k| on[k] && !on[(k + on.len() - 1) % on.len()])
                .collect();
            if starts.len() != 1 || base.is_some() {
                return Err(invalid(
                    "root fillet: the round wall meets the base in more than one run",
                ));
            }
            base = Some((i, w, n, starts[0], len));
        }
    }
    let Some((bf, wire_idx, n, k0, len)) = base else {
        return Err(invalid(
            "root fillet: the round wall does not stand on a planar face perpendicular to its \
             axis at the given point",
        ));
    };
    let base_origin = plane_of(&sh.faces[bf]).map_or(c0, |x| x.0);
    let centre = c0 - n * (c0 - base_origin).dot(n);
    let wire = sh.faces[bf].boundaries[wire_idx].clone();
    let wn = wire.len();
    let at = |k: usize| wire[k % wn];
    let ends_of = |e: usize, o: bool| {
        let (p, q) = sh.edges[e].vertices;
        if o { (p, q) } else { (q, p) }
    };
    let run: Vec<(usize, bool)> = (0..len)
        .map(|j| at(k0 + j))
        .map(|x| (x.index, x.orientation))
        .collect();
    let lset: BTreeSet<usize> = run.iter().map(|x| x.0).collect();
    let lverts = edge_vertices(&sh, &lset);
    let v_start = ends_of(run[0].0, run[0].1).0;
    let v_end = ends_of(run[len - 1].0, run[len - 1].1).1;
    if v_start == v_end {
        return Err(invalid("root fillet: the run of wall edges is closed"));
    }
    // Base edges before and after the run, at its ends.
    let before = at(k0 + wn - 1).index;
    let after = at(k0 + len).index;
    let vf = vertex_faces(&sh);
    let ef = edge_faces(&sh);
    for v in &lverts {
        if *v == v_start || *v == v_end {
            continue;
        }
        if vf[*v].iter().any(|g| *g != bf && !cyl.contains(g)) {
            return Err(invalid(
                "root fillet: other faces meet the wall's base edge (a feature already there)",
            ));
        }
    }
    // End faces: the third face at each end vertex, radial and planar, with
    // the base edge straight and running away from the fillet side.
    let mut ends = Vec::new();
    for (v, be) in [(v_start, before), (v_end, after)] {
        let others: Vec<usize> = vf[v]
            .iter()
            .copied()
            .filter(|g| *g != bf && !cyl.contains(g))
            .collect();
        let [pf] = others[..] else {
            return Err(invalid(
                "root fillet: the run's end must meet exactly one end face (the wall ends \
                 free, not against another wall)",
            ));
        };
        let p = sh.vertices[v];
        let u = {
            let d = p - centre;
            (d - a * d.dot(a)).normalize()
        };
        let radial_plane = plane_of(&sh.faces[pf]).is_some_and(|(o, m)| {
            m.dot(n).abs() <= 1e-9 && m.dot(u).abs() <= 1e-9 && (p - o).dot(m).abs() <= tol
        });
        if !radial_plane {
            return Err(invalid(
                "root fillet: the wall's end face is not a plane through the axis",
            ));
        }
        if !ef[be].contains(&pf) {
            return Err(invalid(
                "root fillet: the end face does not meet the base next to the wall",
            ));
        }
        let (bp, bq) = sh.edges[be].vertices;
        let w = if bp == v { bq } else { bp };
        let dir = sh.vertices[w] - p;
        let l = dir.magnitude();
        let straight = curve_samples(&sh.edges[be].curve, 8)?
            .iter()
            .all(|x| ((*x - p) - dir * ((*x - p).dot(dir) / (l * l))).magnitude() <= tol);
        if l < tol::MIN_LENGTH || !straight || (dir / l + u * s).magnitude() > 1e-7 {
            return Err(invalid(
                "root fillet: the end face's base edge is not a straight radial segment \
                 away from the fillet",
            ));
        }
        ends.push((v, be, pf));
    }
    // Clearance on the base face: no other edge within the fillet's band.
    {
        let fr = frame_on(base_origin, n);
        let c2 = to2(&fr, centre);
        let mid = |e: usize| -> Result<[f64; 2]> {
            let c = &sh.edges[e].curve;
            let (t0, t1) = c.range_tuple();
            Ok(to2(&fr, eval_curve(c, 0.5 * (t0 + t1))?))
        };
        let ang = |p: [f64; 2]| (p[1] - c2[1]).atan2(p[0] - c2[0]);
        let wrap = |x: f64| {
            let t = std::f64::consts::TAU;
            (x + std::f64::consts::PI).rem_euclid(t) - std::f64::consts::PI
        };
        // Angular span of the run, counter-clockwise from `lo`.
        let p_s = to2(&fr, sh.vertices[v_start]);
        let mut sweep = 0.0;
        for &(e, o) in &run {
            let (p, q) = ends_of(e, o);
            let (a0, am, a1) = (
                ang(to2(&fr, sh.vertices[p])),
                ang(mid(e)?),
                ang(to2(&fr, sh.vertices[q])),
            );
            sweep += wrap(am - a0) + wrap(a1 - am);
        }
        let (lo, span) = if sweep >= 0.0 {
            (ang(p_s), sweep)
        } else {
            (ang(p_s) + sweep, -sweep)
        };
        // The band: radii r_in..r_out (widened by the minimum length, so a
        // touching edge counts) over the run's angles.
        let m = tol::MIN_LENGTH;
        let (r_in, r_out) = (r.min(big) - m, r.max(big) + m);
        let band = Band {
            c: c2,
            r_in,
            r_out,
            lo,
            span,
        };
        for (wi, wr) in sh.faces[bf].boundaries.iter().enumerate() {
            for ei in wr {
                if lset.contains(&ei.index)
                    || (wi == wire_idx && (ei.index == before || ei.index == after))
                {
                    continue;
                }
                let Some(e2) = edge2(&sh, ei.index, ei.orientation, &fr, tol)? else {
                    return Err(invalid(
                        "root fillet: the base face has a free-form edge; clearance cannot be \
                         checked exactly",
                    ));
                };
                if band.meets(&Prim::of(&e2)) {
                    return Err(invalid(format!(
                        "root fillet R{f} mm does not fit on the base face: another edge \
                         crosses the fillet's band"
                    )));
                }
            }
        }
    }
    // Wall edges along the axis from the run: shortened by f at the bottom.
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
                "root fillet: a wall edge joins two of the base run's vertices",
            ));
        }
        let (fixed, moving) = if on_p { (q, p) } else { (p, q) };
        let (f0, m0) = (sh.vertices[fixed], sh.vertices[moving]);
        let up = (f0 - m0).dot(n);
        let g = (f0 - m0).normalize();
        if g.cross(n).magnitude() > 1e-9
            || curve_samples(&sh.edges[e].curve, 16)?
                .iter()
                .any(|x| ((*x - m0) - g * (*x - m0).dot(g)).magnitude() > tol)
        {
            return Err(invalid(
                "root fillet: the wall has an edge that is not a straight generator",
            ));
        }
        if up - f < tol::MIN_LENGTH {
            return Err(invalid(format!(
                "root fillet R{f} mm reaches the top of the wall ({up:.6} mm high)"
            )));
        }
        seams.insert(e);
    }
    for &(v, _, pf) in &ends {
        let walls = seams
            .iter()
            .filter(|e| {
                let (p, q) = sh.edges[**e].vertices;
                (p == v || q == v) && ef[**e].contains(&pf)
            })
            .count();
        if walls != 1 {
            return Err(invalid(
                "root fillet: the end face does not meet the wall in one straight edge",
            ));
        }
    }
    let lift = Matrix4::from_translation(n * f);
    let scale_out = radial_scaling(centre, a, big / r);
    let w45 = std::f64::consts::FRAC_PI_4.cos();
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
    let h4 = |p: Point3, w: f64| Vector4::new(p.x * w, p.y * w, p.z * w, w);
    // Quarter circles at the run's vertices: lifted vertex -> corner -> new
    // base vertex.
    let mut quarter = BTreeMap::new();
    for &v in &lverts {
        quarter.insert(v, sh.edges.len());
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
    let mut touched: BTreeSet<usize> = seams.clone();
    touched.extend(quarter.values().copied());
    use truck_topology::compress::CompressedEdgeIndex as Ix;
    // Ends: the base edge runs from the new base vertex; the end face gains
    // the quarter circle between its wall edge and its base edge.
    for &(v, be, pf) in &ends {
        let e = &mut sh.edges[be];
        if e.vertices.0 == v {
            e.vertices.0 = out_v[&v];
        } else {
            e.vertices.1 = out_v[&v];
        }
        let (p, q) = e.vertices;
        e.curve = Curve::Line(Line(sh.vertices[p], sh.vertices[q]));
        touched.insert(be);
        let qe = quarter[&v];
        let mut placed = false;
        for wr in &mut sh.faces[pf].boundaries {
            let m = wr.len();
            for k in 0..m {
                let (x, y) = (wr[k], wr[(k + 1) % m]);
                // x ends at v (wall edge, then base edge) or at the new
                // vertex (base edge, then wall edge).
                if seams.contains(&x.index) && y.index == be {
                    wr.insert(
                        k + 1,
                        Ix {
                            index: qe,
                            orientation: true,
                        },
                    );
                    placed = true;
                } else if x.index == be && seams.contains(&y.index) {
                    wr.insert(
                        k + 1,
                        Ix {
                            index: qe,
                            orientation: false,
                        },
                    );
                    placed = true;
                }
                if placed {
                    break;
                }
            }
            if placed {
                break;
            }
        }
        if !placed {
            return Err(invalid(
                "root fillet: the end face's wall and base edges are not adjacent",
            ));
        }
    }
    let base_flip = !sh.faces[bf].orientation;
    let profile = [(r, f, 1.0), (r, 0.0, w45), (big, 0.0, 1.0)];
    let nurbs = |c: &Curve| -> Result<NurbsCurve<Vector4>> {
        match c {
            Curve::NurbsCurve(c) => Ok(c.clone()),
            Curve::BSplineCurve(c) => Ok(NurbsCurve::from(c.clone())),
            _ => Err(invalid(
                "root fillet: the wall's base edge is not a (rational) B-spline arc",
            )),
        }
    };
    let mut sweep_total = 0.0;
    for (j, &(e, o)) in run.iter().enumerate() {
        let original = sh.edges[e].curve.clone();
        let src = nurbs(&original)?;
        {
            let (t0, t1) = original.range_tuple();
            let (p, q) = sh.edges[e].vertices;
            let rad = |x: Point3| {
                let d = x - centre;
                (d - a * d.dot(a)).normalize()
            };
            let (u0, um, u1) = (
                rad(corner[&p]),
                rad(eval_curve(&original, 0.5 * (t0 + t1))?),
                rad(corner[&q]),
            );
            sweep_total += u0.angle(um).0 + um.angle(u1).0;
        }
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
        sh.faces[bf].boundaries[wire_idx][(k0 + j) % wn].index = eu;
        touched.extend([e, eu]);
        let o = o != base_flip;
        let (sv, tv) = if o { (va, vb) } else { (vb, va) };
        let wire = vec![
            Ix {
                index: e,
                orientation: o,
            },
            Ix {
                index: quarter[&tv],
                orientation: true,
            },
            Ix {
                index: eu,
                orientation: !o,
            },
            Ix {
                index: quarter[&sv],
                orientation: false,
            },
        ];
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
        // centres at radius r + s f, height f.
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
    // Probes at the middle of the first arc: in the fillet beside the wall,
    // and in the air near the fillet circle's centre.
    let (e0, _) = run[0];
    let pm = {
        let c = &shell_of(body)?.edges[e0].curve;
        let (t0, t1) = c.range_tuple();
        eval_curve(c, 0.5 * (t0 + t1))?
    };
    let u = {
        let d = pm - centre;
        (d - a * d.dot(a)).normalize()
    };
    let material = pm + u * (s * 0.1 * f) + n * (0.05 * f);
    let air = pm + u * (s * 0.8 * f) + n * (0.8 * f);
    let m = crate::point_membership(&out, &[p3(material), p3(air)])?;
    if m[0] != Some(true) || m[1] != Some(false) {
        return Err(KernelError::InvalidResult(format!(
            "root fillet failed the probe check (material {:?}, air {:?})",
            m[0], m[1]
        )));
    }
    // Added volume (Pappus): the spandrel f²(1 − π/4) revolved through the
    // run's sweep about the axis, its centroid r + s·d from it.
    let pi = std::f64::consts::PI;
    let area = f * f * (1.0 - pi / 4.0);
    let d = f * (10.0 - 3.0 * pi) / (12.0 - 3.0 * pi);
    let want = sweep_total * area * (r + s * d);
    let vol = |b: &Body| {
        crate::measure::verified_volume(b).map_err(|reason| {
            KernelError::InvalidResult(format!(
                "root fillet: cannot verify the added volume ({reason})"
            ))
        })
    };
    let ((v0, e0), (v1, e1)) = (vol(body)?, vol(&out)?);
    let tol_v = (1e-7 * v0).max(1e-6) + 3.0 * (e0 + e1);
    if ((v1 - v0) - want).abs() > tol_v {
        return Err(KernelError::InvalidResult(format!(
            "root fillet: added {:.9} mm³, expected {want:.9} mm³",
            v1 - v0
        )));
    }
    Ok(out)
}

/// A segment or a counter-clockwise circular arc in a plane frame.
#[derive(Debug, Clone, Copy)]
enum Prim {
    Seg([f64; 2], [f64; 2]),
    Arc {
        c: [f64; 2],
        r: f64,
        a0: f64,
        sweep: f64,
    },
}

fn angle_of(c: [f64; 2], p: [f64; 2]) -> f64 {
    (p[1] - c[1]).atan2(p[0] - c[0])
}

/// Angle `t` within `[a0, a0 + sweep]` (counter-clockwise), to 1e-12.
fn within(t: f64, a0: f64, sweep: f64) -> bool {
    let rel = (t - a0).rem_euclid(std::f64::consts::TAU);
    rel <= sweep + 1e-12 || rel >= std::f64::consts::TAU - 1e-12
}

impl Prim {
    fn of(e: &Edge2) -> Prim {
        match *e {
            Edge2::Line(a, b) => Prim::Seg(a, b),
            Edge2::Arc { a, b, center, ccw } => {
                let sweep = crate::arc_signed_sweep(a, b, center, ccw);
                let a0 = angle_of(center, a);
                let r = (a[0] - center[0]).hypot(a[1] - center[1]);
                if sweep >= 0.0 {
                    Prim::Arc {
                        c: center,
                        r,
                        a0,
                        sweep,
                    }
                } else {
                    Prim::Arc {
                        c: center,
                        r,
                        a0: a0 + sweep,
                        sweep: -sweep,
                    }
                }
            }
        }
    }

    fn start(&self) -> [f64; 2] {
        match *self {
            Prim::Seg(a, _) => a,
            Prim::Arc { c, r, a0, .. } => [c[0] + r * a0.cos(), c[1] + r * a0.sin()],
        }
    }

    fn on_arc(&self, p: [f64; 2]) -> bool {
        match *self {
            Prim::Seg(..) => true,
            Prim::Arc { c, a0, sweep, .. } => within(angle_of(c, p), a0, sweep),
        }
    }

    /// Whether two primitives share a point (exact up to rounding).
    fn meets(&self, other: &Prim) -> bool {
        match (*self, *other) {
            (Prim::Seg(p, q), Prim::Seg(u, v)) => seg_seg(p, q, u, v),
            (Prim::Seg(p, q), arc @ Prim::Arc { c, r, .. })
            | (arc @ Prim::Arc { c, r, .. }, Prim::Seg(p, q)) => {
                seg_circle(p, q, c, r).into_iter().any(|x| arc.on_arc(x))
            }
            (
                Prim::Arc {
                    c: c1,
                    r: r1,
                    a0: a1,
                    sweep: s1,
                },
                Prim::Arc {
                    c: c2,
                    r: r2,
                    a0: a2,
                    sweep: s2,
                },
            ) => {
                let d = (c2[0] - c1[0]).hypot(c2[1] - c1[1]);
                if d <= 1e-12 * (1.0 + r1 + r2) {
                    // Concentric: only the same circle, where the angle
                    // ranges overlap.
                    return (r1 - r2).abs() <= 1e-12 * (1.0 + r1)
                        && (within(a2, a1, s1) || within(a1, a2, s2));
                }
                if d > r1 + r2 || d < (r1 - r2).abs() {
                    return false;
                }
                let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let (ux, uy) = ((c2[0] - c1[0]) / d, (c2[1] - c1[1]) / d);
                let base = [c1[0] + x * ux, c1[1] + x * uy];
                [
                    [base[0] - h * uy, base[1] + h * ux],
                    [base[0] + h * uy, base[1] - h * ux],
                ]
                .into_iter()
                .any(|p| within(angle_of(c1, p), a1, s1) && within(angle_of(c2, p), a2, s2))
            }
        }
    }
}

fn seg_seg(p: [f64; 2], q: [f64; 2], u: [f64; 2], v: [f64; 2]) -> bool {
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let (d1, d2, w) = (sub(q, p), sub(v, u), sub(u, p));
    let den = cross(d1, d2);
    let scale = 1e-12 * (1.0 + d1[0].abs() + d1[1].abs()) * (1.0 + d2[0].abs() + d2[1].abs());
    if den.abs() <= scale {
        // Parallel: overlapping only when collinear.
        if cross(w, d1).abs() > scale {
            return false;
        }
        let l2 = d1[0] * d1[0] + d1[1] * d1[1];
        let t = |x: [f64; 2]| {
            let s = sub(x, p);
            (s[0] * d1[0] + s[1] * d1[1]) / l2
        };
        let (t0, t1) = (t(u), t(v));
        return t0.min(t1) <= 1.0 && t0.max(t1) >= 0.0;
    }
    let (t, s) = (cross(w, d2) / den, cross(w, d1) / den);
    (-1e-12..=1.0 + 1e-12).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&s)
}

/// Points of segment `p q` on the circle about `c` of radius `r`.
fn seg_circle(p: [f64; 2], q: [f64; 2], c: [f64; 2], r: f64) -> Vec<[f64; 2]> {
    let d = [q[0] - p[0], q[1] - p[1]];
    let f = [p[0] - c[0], p[1] - c[1]];
    let a = d[0] * d[0] + d[1] * d[1];
    let b = 2.0 * (f[0] * d[0] + f[1] * d[1]);
    let k = f[0] * f[0] + f[1] * f[1] - r * r;
    let disc = b * b - 4.0 * a * k;
    if a <= 0.0 || disc < 0.0 {
        return Vec::new();
    }
    let sq = disc.sqrt();
    [(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)]
        .into_iter()
        .filter(|t| (-1e-12..=1.0 + 1e-12).contains(t))
        .map(|t| [p[0] + t * d[0], p[1] + t * d[1]])
        .collect()
}

/// The fillet's band on the base: an annular sector.
struct Band {
    c: [f64; 2],
    r_in: f64,
    r_out: f64,
    lo: f64,
    span: f64,
}

impl Band {
    fn contains(&self, p: [f64; 2]) -> bool {
        let rho = (p[0] - self.c[0]).hypot(p[1] - self.c[1]);
        rho > self.r_in && rho < self.r_out && within(angle_of(self.c, p), self.lo, self.span)
    }

    /// Whether an edge reaches into the band: it starts inside it or
    /// crosses one of its four sides.
    fn meets(&self, e: &Prim) -> bool {
        let ray = |r: f64, t: f64| [self.c[0] + r * t.cos(), self.c[1] + r * t.sin()];
        let hi = self.lo + self.span;
        let sides = [
            Prim::Arc {
                c: self.c,
                r: self.r_in,
                a0: self.lo,
                sweep: self.span,
            },
            Prim::Arc {
                c: self.c,
                r: self.r_out,
                a0: self.lo,
                sweep: self.span,
            },
            Prim::Seg(ray(self.r_in, self.lo), ray(self.r_out, self.lo)),
            Prim::Seg(ray(self.r_in, hi), ray(self.r_out, hi)),
        ];
        self.contains(e.start()) || sides.iter().any(|s| s.meets(e))
    }
}

#[cfg(test)]
mod band_tests {
    use super::*;
    use std::f64::consts::PI;

    /// Upper half of the annulus 9..10 about the origin.
    fn band() -> Band {
        Band {
            c: [0.0, 0.0],
            r_in: 9.0,
            r_out: 10.0,
            lo: 0.0,
            span: PI,
        }
    }

    #[test]
    fn edges_inside_crossing_and_clear() {
        let b = band();
        // Wholly inside, crossing the outer arc, crossing an end, clear
        // below, and passing over the annulus' hole without touching it.
        assert!(b.meets(&Prim::Seg([-0.5, 9.5], [0.5, 9.5])));
        assert!(b.meets(&Prim::Seg([0.0, 9.5], [0.0, 12.0])));
        assert!(b.meets(&Prim::Seg([9.5, -1.0], [9.5, 1.0])));
        assert!(!b.meets(&Prim::Seg([-12.0, -0.5], [12.0, -0.5])));
        assert!(!b.meets(&Prim::Seg([-5.0, 3.0], [5.0, 3.0])));
        // A line through the band's chord region inside r_in: clear.
        assert!(!b.meets(&Prim::Seg([-8.0, 4.0], [8.0, 4.0])));
    }

    #[test]
    fn arcs_concentric_and_crossing() {
        let b = band();
        let arc = |c: [f64; 2], r: f64, a0: f64, sweep: f64| Prim::Arc { c, r, a0, sweep };
        // Concentric inside the band's angles; concentric but below; a
        // circle of another centre cutting through the outer side.
        assert!(b.meets(&arc([0.0, 0.0], 9.5, 0.2, 0.1)));
        assert!(!b.meets(&arc([0.0, 0.0], 9.5, PI + 0.2, 0.5)));
        assert!(b.meets(&arc([0.0, 12.0], 2.5, PI, PI)));
        assert!(!b.meets(&arc([0.0, 13.0], 2.5, 0.0, 2.0 * PI)));
    }
}
