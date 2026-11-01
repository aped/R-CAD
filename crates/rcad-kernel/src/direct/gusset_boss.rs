//! A round boss (cylinder, or a cone narrowing by a draft) with planar
//! gusset ribs, standing on a planar face, built directly into the
//! topology: no boolean, so no plane/cone face classification.
//!
//! In the boss frame (origin on the base face's plane on the axis, z the
//! base's outward normal, angles from x), the shank has radius `R(z) = R0 −
//! z tan(draft)` up to `height`. Gusset `k` is the slab `|X·p| ≤ w` (`w` half
//! the thickness, `e` the radial direction at its angle, `p` the
//! perpendicular) outside the shank, bounded by its side outline in
//! (radius, height): the base out to `length`, the end face up to
//! `tip_height`, a chamfer to `(length − chamfer, rib_height)`, the flat top
//! back to the shank. The pieces meet in exact edges:
//!
//! * a side plane `X·p = ±w` meets the cone in a hyperbola (a straight
//!   generator for a cylinder), a rational quadratic through the end points
//!   with the tangents' intersection as middle point and the weight from the
//!   curve's shoulder point;
//! * the rib's top plane meets the cone in a circle arc, the base face in
//!   arcs and lines; the cone is split into one panel over each gusset (a
//!   generator from each top corner up to the shank top) and one panel
//!   between neighbouring gussets, each the exact tensor product of a
//!   rational arc with the generator.
//!
//! The base face gains the footprint (shank arcs and gusset outlines) as an
//! inner loop. Preconditions (refused with the reason): the footprint lies
//! inside the base face, farther than `MIN_LENGTH` from its other edges; no
//! other face reaches into the boss's bounding box above the base; the ribs
//! fit (narrower than the shank at their top, not meeting each other on it,
//! below the shank top, starting outside it). Checked like every direct
//! edit, plus probes and the added volume against a one-dimensional
//! integration of the cross-section areas (independent of the topology).
use super::*;
use truck_topology::compress::{CompressedEdge, CompressedEdgeIndex as Ix};

/// Shape of a gusseted boss (lengths in mm, angles in radians).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GussetBoss {
    /// Shank diameter at the base.
    pub base_diameter: f64,
    pub height: f64,
    /// Draft of the shank wall (0: a cylinder); it narrows away from the base.
    pub draft: f64,
    /// Gusset directions, from the frame's x axis towards its y axis.
    pub angles: Vec<f64>,
    pub thickness: f64,
    /// Radial reach of the gussets from the axis.
    pub length: f64,
    /// Radial length of the chamfer from the end down to the tip (0: none).
    pub chamfer: f64,
    pub rib_height: f64,
    pub tip_height: f64,
}

/// Rational arc about the frame's z axis as unit-radius control points
/// `(cos, sin, weight)` with its knot vector (segments of at most 90°).
fn unit_arc(phi0: f64, phi1: f64) -> (KnotVec, Vec<(f64, f64, f64)>) {
    let n = ((phi1 - phi0) / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let d = (phi1 - phi0) / n as f64;
    let wm = (d / 2.0).cos();
    let mut pts = vec![(phi0.cos(), phi0.sin(), 1.0)];
    let mut knots = vec![0.0, 0.0, 0.0];
    for i in 0..n {
        let a = phi0 + d * i as f64;
        let m = a + d / 2.0;
        pts.push((m.cos() / wm, m.sin() / wm, wm));
        pts.push(((a + d).cos(), (a + d).sin(), 1.0));
        let k = (i + 1) as f64;
        if i + 1 < n {
            knots.extend([k, k]);
        } else {
            knots.extend([k, k, k]);
        }
    }
    (KnotVec::from(knots), pts)
}

fn h4(p: Point3, w: f64) -> Vector4 {
    Vector4::new(p.x * w, p.y * w, p.z * w, w)
}

/// How a face's loop is unrolled to decide its sense: a plane basis with
/// `a × b` the outward normal, or (angle about the axis, height) on the
/// shank, whose outward normal points away from the axis.
#[derive(Clone, Copy)]
enum Chart {
    Plane(Point3, Vector3, Vector3),
    Shank,
}

pub fn gusset_boss(body: &Body, frame: &Frame, spec: &GussetBoss) -> Result<Body> {
    frame.validate()?;
    let GussetBoss {
        base_diameter,
        height: h,
        draft,
        ref angles,
        thickness,
        length: rl,
        chamfer,
        rib_height: rh,
        tip_height: tip,
    } = *spec;
    let finite = [base_diameter, h, draft, thickness, rl, chamfer, rh, tip]
        .iter()
        .chain(angles)
        .all(|v| v.is_finite());
    if !finite {
        return Err(invalid("gusseted boss: non-finite input"));
    }
    if !(0.0..std::f64::consts::FRAC_PI_4).contains(&draft) {
        return Err(invalid(
            "gusseted boss: the shank's draft must be within 0..45 degrees",
        ));
    }
    if angles.is_empty() {
        return Err(invalid("gusseted boss: no gussets"));
    }
    let m = tol::MIN_LENGTH;
    let (r0, t) = (base_diameter / 2.0, draft.tan());
    let w = thickness / 2.0;
    let rad = |z: f64| r0 - z * t;
    let r1 = rad(h);
    let rhh = rad(rh);
    if r0 < m || h < m || w < m / 2.0 || chamfer < 0.0 || tip < 0.0 {
        return Err(invalid(
            "gusseted boss: diameter, height and thickness must be positive",
        ));
    }
    if r1 < m {
        return Err(invalid(
            "gusseted boss: the draft closes the shank before its top",
        ));
    }
    if rh < m || rh > h - m {
        return Err(invalid(format!(
            "gusseted boss: the ribs must end below the shank top (rib height {rh} mm, \
             shank {h} mm)"
        )));
    }
    if tip > rh + m {
        return Err(invalid("gusseted boss: the rib tip is higher than the rib"));
    }
    if rhh <= w + m {
        return Err(invalid(format!(
            "gusseted boss: the ribs ({thickness} mm) are not narrower than the shank at their \
             top ({:.6} mm)",
            2.0 * rhh
        )));
    }
    let s_at = |z: f64| (rad(z).powi(2) - w * w).sqrt();
    let (s0, s_h) = (s_at(0.0), s_at(rh));
    let a0 = w.atan2(s0);
    let ah = w.atan2(s_h);
    let has_chamfer = chamfer > m && rh - tip > m;
    if rl <= s0 + m || (has_chamfer && rl - chamfer <= s0 + m) {
        return Err(invalid(
            "gusseted boss: the ribs (and their chamfer) must start outside the shank",
        ));
    }
    // Gusset angles, sorted in [0, 2π), and their gaps on the shank.
    let tau = std::f64::consts::TAU;
    let mut th: Vec<f64> = angles.iter().map(|a| a.rem_euclid(tau)).collect();
    th.sort_by(f64::total_cmp);
    let nr = th.len();
    for k in 0..nr {
        let next = if k + 1 < nr { th[k + 1] } else { th[0] + tau };
        let gap = next - th[k];
        if nr > 1 && gap <= 2.0 * ah + 1e-6 {
            return Err(invalid(format!(
                "gusseted boss: neighbouring ribs meet on the shank ({:.3}° apart, they need \
                 more than {:.3}°)",
                gap.to_degrees(),
                (2.0 * ah).to_degrees()
            )));
        }
    }
    // Side outline above the base from the end outwards: (radius, height).
    let mut outline = vec![(rl, 0.0)];
    if has_chamfer {
        if tip > m {
            outline.push((rl, tip));
        }
        outline.push((rl - chamfer, rh));
    } else {
        outline.push((rl, rh));
    }
    let reach = r0.max(rl.hypot(w));

    let mut sh_ = shell_of(body)?;
    let tol = geo_tol(&sh_);
    let o = Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]);
    let vx = Vector3::new(frame.x[0], frame.x[1], frame.x[2]).normalize();
    let vy = Vector3::new(frame.y[0], frame.y[1], frame.y[2]).normalize();
    let vz = Vector3::new(frame.z[0], frame.z[1], frame.z[2]).normalize();
    let local = |p: Point3| {
        let d = p - o;
        (d.dot(vx), d.dot(vy), d.dot(vz))
    };
    // Base face: a plane through the origin with outward normal z whose
    // region holds the axis.
    let fr = frame_on(o, vz);
    let mut base = None;
    for (i, face) in sh_.faces.iter().enumerate() {
        let Some((po, n)) = plane_of(face) else {
            continue;
        };
        if n.dot(vz) < 1.0 - 1e-9 || (po - o).dot(vz).abs() > tol {
            continue;
        }
        // Even-odd test of the origin against the face's sampled wires.
        let mut inside = false;
        for wire in &face.boundaries {
            let mut poly = Vec::new();
            for ei in wire {
                let mut s = curve_samples(&sh_.edges[ei.index].curve, 32)?;
                if !ei.orientation {
                    s.reverse();
                }
                poly.extend(s.into_iter().map(|p| to2(&fr, p)));
            }
            for j in 0..poly.len() {
                let (a, b) = (poly[j], poly[(j + 1) % poly.len()]);
                if (a[1] > 0.0) != (b[1] > 0.0) {
                    let x = a[0] + (0.0 - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                    if x > 0.0 {
                        inside = !inside;
                    }
                }
            }
        }
        if inside {
            if base.is_some() {
                return Err(invalid("gusseted boss: two base faces hold the axis"));
            }
            base = Some(i);
        }
    }
    let Some(bf) = base else {
        return Err(invalid(
            "gusseted boss: no planar face of the body facing along the axis holds the boss's \
             position",
        ));
    };
    // Exact clearance on the base face: every edge farther than the
    // footprint's reach from the axis.
    for wire in &sh_.faces[bf].boundaries {
        for ei in wire {
            let Some(e2) = edge2(&sh_, ei.index, ei.orientation, &fr, tol)? else {
                return Err(invalid(
                    "gusseted boss: the base face has a free-form edge; clearance cannot be \
                     checked exactly",
                ));
            };
            let d = dist2([0.0, 0.0], &e2);
            if d <= reach + m {
                return Err(invalid(format!(
                    "gusseted boss does not fit on the base face: an edge is {d:.6} mm from the \
                     axis, within the ribs' reach {reach:.6} mm"
                )));
            }
        }
    }
    // Nothing else above the base within the boss's bounding box (face
    // bounding boxes from boundary samples, widened for curved faces).
    for (i, face) in sh_.faces.iter().enumerate() {
        if i == bf {
            continue;
        }
        let mut pts: Vec<Point3> = Vec::new();
        for e in face_edges(face) {
            pts.extend(curve_samples(&sh_.edges[e].curve, 32)?);
        }
        let curved = !matches!(face.surface, Surface::Plane(_));
        if curved && let Ok(s) = face_samples(&sh_, face) {
            pts.extend(s.into_iter().map(|x| x.0));
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &pts {
            let (x, y, z) = local(*p);
            for (k, v) in [x, y, z].into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
        let pad = if curved {
            0.02 * (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max)
        } else {
            0.0
        };
        let overlaps = lo[0] - pad < reach
            && hi[0] + pad > -reach
            && lo[1] - pad < reach
            && hi[1] + pad > -reach
            && lo[2] - pad < h
            && hi[2] + pad > tol;
        if overlaps {
            return Err(invalid(
                "gusseted boss: another face of the body reaches into the space the boss \
                 would fill",
            ));
        }
    }

    // ------------------------------------------------------------ build
    let pt = |x: f64, y: f64, z: f64| o + vx * x + vy * y + vz * z;
    let polar = |r: f64, phi: f64, z: f64| pt(r * phi.cos(), r * phi.sin(), z);
    let sh = &mut sh_;
    let add_v = |sh: &mut CS, p: Point3| {
        sh.vertices.push(p);
        sh.vertices.len() - 1
    };
    let add_e = |sh: &mut CS, a: usize, b: usize, curve: Curve| {
        sh.edges.push(CompressedEdge {
            vertices: (a, b),
            curve,
        });
        sh.edges.len() - 1
    };
    let line = |sh: &mut CS, a: usize, b: usize| {
        let c = Curve::Line(Line(sh.vertices[a], sh.vertices[b]));
        add_e(sh, a, b, c)
    };
    let arc_curve = |r: f64, z: f64, phi0: f64, phi1: f64| {
        let (knots, unit) = unit_arc(phi0, phi1);
        let cps = unit
            .iter()
            .map(|&(c, s, wt)| h4(pt(r * c, r * s, z), wt))
            .collect();
        Curve::NurbsCurve(NurbsCurve::new(BSplineCurve::new(knots, cps)))
    };
    struct Gusset {
        hyp: [usize; 2],
        // Outline edges per side: base, outline segments, top.
        side: [Vec<usize>; 2],
        cross: Vec<usize>,
        arc_t: usize,
        gen_: [usize; 2],
        top_cap: usize,
        b: [usize; 2],
        u: [usize; 2],
    }
    let mut gs: Vec<Gusset> = Vec::with_capacity(nr);
    for &theta in &th {
        let e = vx * theta.cos() + vy * theta.sin();
        let p = -vx * theta.sin() + vy * theta.cos();
        let at = |s: f64, sigma: f64, z: f64| o + e * s + p * (sigma * w) + vz * z;
        let mut hyp = [0; 2];
        let mut side: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        let mut b = [0; 2];
        let mut tv = [0; 2];
        let mut qv: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        let mut gen_ = [0; 2];
        let mut u = [0; 2];
        for (si, sigma) in [-1.0, 1.0].into_iter().enumerate() {
            b[si] = add_v(sh, at(s0, sigma, 0.0));
            tv[si] = add_v(sh, at(s_h, sigma, rh));
            for &(r, z) in &outline {
                qv[si].push(add_v(sh, at(r, sigma, z)));
            }
            // Side plane ∩ cone: s² = R(z)² − w².
            let curve = if t < 1e-12 {
                Curve::Line(Line(sh.vertices[b[si]], sh.vertices[tv[si]]))
            } else {
                let (p0, p2) = ([s0, 0.0], [s_h, rh]);
                // Tangent (ds, dz) = (−t R / s, 1).
                let d0 = [-t * rad(0.0) / p0[0], 1.0];
                let d2 = [-t * rad(rh) / p2[0], 1.0];
                let cr = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
                let den = cr(d0, d2);
                if den.abs() < 1e-15 {
                    return Err(invalid("gusseted boss: degenerate rib/shank edge"));
                }
                let diff = [p2[0] - p0[0], p2[1] - p0[1]];
                let a = cr(diff, d2) / den;
                let p1 = [p0[0] + a * d0[0], p0[1] + a * d0[1]];
                let mid = [(p0[0] + p2[0]) / 2.0, (p0[1] + p2[1]) / 2.0];
                let f = |l: f64| {
                    let s = mid[0] + l * (p1[0] - mid[0]);
                    let z = mid[1] + l * (p1[1] - mid[1]);
                    s * s - rad(z).powi(2) + w * w
                };
                // F is quadratic along the line: fit it from three values.
                let (c0, ch_, c1) = (f(0.0), f(0.5), f(1.0));
                let qa = 2.0 * c1 - 4.0 * ch_ + 2.0 * c0;
                let qb = 4.0 * ch_ - 3.0 * c0 - c1;
                let qc = c0;
                let roots: Vec<f64> = if qa.abs() < 1e-300 {
                    vec![-qc / qb]
                } else {
                    let disc = (qb * qb - 4.0 * qa * qc).max(0.0).sqrt();
                    vec![(-qb - disc) / (2.0 * qa), (-qb + disc) / (2.0 * qa)]
                };
                let Some(lam) = roots.into_iter().find(|l| *l > 0.0 && *l < 1.0) else {
                    return Err(invalid(
                        "gusseted boss: rib/shank edge has no shoulder point",
                    ));
                };
                let wt = lam / (1.0 - lam);
                let cps = vec![
                    h4(at(p0[0], sigma, p0[1]), 1.0),
                    h4(at(p1[0], sigma, p1[1]), wt),
                    h4(at(p2[0], sigma, p2[1]), 1.0),
                ];
                Curve::NurbsCurve(NurbsCurve::new(BSplineCurve::new(
                    KnotVec::bezier_knot(2),
                    cps,
                )))
            };
            hyp[si] = add_e(sh, b[si], tv[si], curve);
            side[si].push(line(sh, b[si], qv[si][0]));
            for j in 0..qv[si].len() - 1 {
                side[si].push(line(sh, qv[si][j], qv[si][j + 1]));
            }
            let last = *qv[si].last().unwrap_or(&qv[si][0]);
            side[si].push(line(sh, last, tv[si]));
            let phi = theta + sigma * ah;
            u[si] = add_v(sh, polar(r1, phi, h));
            gen_[si] = line(sh, tv[si], u[si]);
        }
        let cross: Vec<usize> = (0..outline.len())
            .map(|j| line(sh, qv[0][j], qv[1][j]))
            .collect();
        let arc_t = add_e(sh, tv[0], tv[1], arc_curve(rhh, rh, theta - ah, theta + ah));
        let top_cap = add_e(sh, u[0], u[1], arc_curve(r1, h, theta - ah, theta + ah));
        gs.push(Gusset {
            hyp,
            side,
            cross,
            arc_t,
            gen_,
            top_cap,
            b,
            u,
        });
    }
    // Arcs between neighbours on the base and on the top.
    let mut base_arc = Vec::with_capacity(nr);
    let mut top_arc = Vec::with_capacity(nr);
    for k in 0..nr {
        let kn = (k + 1) % nr;
        let next = if k + 1 < nr { th[k + 1] } else { th[0] + tau };
        let (ba, bb) = (gs[k].b[1], gs[kn].b[0]);
        base_arc.push(add_e(sh, ba, bb, arc_curve(r0, 0.0, th[k] + a0, next - a0)));
        let (ua, ub) = (gs[k].u[1], gs[kn].u[0]);
        top_arc.push(add_e(sh, ua, ub, arc_curve(r1, h, th[k] + ah, next - ah)));
    }

    // ------------------------------------------------------------ faces
    let chain = |sh: &CS, edges: &[usize]| -> Result<Vec<Ix>> {
        let n = edges.len();
        let mut out: Vec<Ix> = Vec::with_capacity(n);
        let (a0, b0) = sh.edges[edges[0]].vertices;
        let (a1, b1) = sh.edges[edges[(1) % n]].vertices;
        let first_fwd = b0 == a1 || b0 == b1;
        out.push(Ix {
            index: edges[0],
            orientation: first_fwd,
        });
        let mut end = if first_fwd { b0 } else { a0 };
        for &e in &edges[1..] {
            let (a, b) = sh.edges[e].vertices;
            let fwd = if a == end {
                true
            } else if b == end {
                false
            } else {
                return Err(KernelError::InvalidResult(
                    "gusseted boss: a face loop does not close".into(),
                ));
            };
            end = if fwd { b } else { a };
            out.push(Ix {
                index: e,
                orientation: fwd,
            });
        }
        let start = if first_fwd { a0 } else { b0 };
        if end != start {
            return Err(KernelError::InvalidResult(
                "gusseted boss: a face loop does not close".into(),
            ));
        }
        Ok(out)
    };
    let reversed = |wire: Vec<Ix>| -> Vec<Ix> {
        wire.into_iter()
            .rev()
            .map(|x| Ix {
                index: x.index,
                orientation: !x.orientation,
            })
            .collect()
    };
    // Signed area of a loop in a chart (positive: counter-clockwise about
    // the outward normal).
    let area = |sh: &CS, wire: &[Ix], chart: Chart| -> Result<f64> {
        let mut pts: Vec<[f64; 2]> = Vec::new();
        for ix in wire {
            let mut s = curve_samples(&sh.edges[ix.index].curve, 16)?;
            if !ix.orientation {
                s.reverse();
            }
            s.pop();
            for p in s {
                let q = match chart {
                    Chart::Plane(c, a, b) => [(p - c).dot(a), (p - c).dot(b)],
                    Chart::Shank => {
                        let (x, y, z) = local(p);
                        let mut phi = y.atan2(x);
                        if let Some(prev) = pts.last() {
                            while phi - prev[0] > std::f64::consts::PI {
                                phi -= tau;
                            }
                            while prev[0] - phi > std::f64::consts::PI {
                                phi += tau;
                            }
                        }
                        [phi, z]
                    }
                };
                pts.push(q);
            }
        }
        let mut a2 = 0.0;
        for j in 0..pts.len() {
            let (p, q) = (pts[j], pts[(j + 1) % pts.len()]);
            a2 += p[0] * q[1] - q[0] * p[1];
        }
        Ok(a2 / 2.0)
    };
    let basis = |n: Vector3| {
        let a = crate::step_in::least_aligned_axis(n);
        let a = (a - n * a.dot(n)).normalize();
        (a, n.cross(a))
    };
    let plane_face = |sh: &mut CS, edges: &[usize], origin: Point3, n: Vector3| -> Result<()> {
        let (a, b) = basis(n);
        let mut wire = chain(sh, edges)?;
        if area(sh, &wire, Chart::Plane(origin, a, b))? < 0.0 {
            wire = reversed(wire);
        }
        sh.faces.push(CompressedFace {
            boundaries: vec![wire],
            orientation: true,
            surface: Surface::Plane(Plane::new(origin, origin + a, origin + b)),
        });
        Ok(())
    };
    let shank_face = |sh: &mut CS, edges: &[usize], phi0: f64, phi1: f64| -> Result<()> {
        let (knots, unit) = unit_arc(phi0, phi1);
        let (u0, u1) = (knots[0], knots[knots.len() - 1]);
        let rows: Vec<Vec<Vector4>> = unit
            .iter()
            .map(|&(c, s, wt)| {
                vec![
                    h4(pt(r0 * c, r0 * s, 0.0), wt),
                    h4(pt(r1 * c, r1 * s, h), wt),
                ]
            })
            .collect();
        let surface = Surface::NurbsSurface(NurbsSurface::new(BSplineSurface::new(
            (knots, KnotVec::bezier_knot(1)),
            rows,
        )));
        let pm = surface.subs(0.5 * (u0 + u1), 0.5);
        let nm = surface.normal(0.5 * (u0 + u1), 0.5);
        let (x, y, _) = local(pm);
        let out = (vx * x + vy * y).normalize();
        let orientation = nm.dot(out) > 0.0;
        let mut wire = chain(sh, edges)?;
        if area(sh, &wire, Chart::Shank)? < 0.0 {
            wire = reversed(wire);
        }
        if !orientation {
            wire = reversed(wire);
        }
        sh.faces.push(CompressedFace {
            boundaries: vec![wire],
            orientation,
            surface,
        });
        Ok(())
    };
    let margin = 0.01f64.min(a0 / 2.0);
    for (k, &theta) in th.iter().enumerate() {
        let e = vx * theta.cos() + vy * theta.sin();
        let p = -vx * theta.sin() + vy * theta.cos();
        let g = &gs[k];
        // Sides (outward ∓p).
        for (si, sigma) in [-1.0, 1.0].into_iter().enumerate() {
            let mut edges = g.side[si].clone();
            edges.push(g.hyp[si]);
            plane_face(sh, &edges, o + p * (sigma * w), p * sigma)?;
        }
        // Outline segments: end face, chamfer.
        for j in 0..outline.len() - 1 {
            let (ra, za) = outline[j];
            let (rb, zb) = outline[j + 1];
            let (dr, dz) = (rb - ra, zb - za);
            let n = (e * dz - vz * dr).normalize();
            let edges = [
                g.cross[j],
                g.side[1][j + 1],
                g.cross[j + 1],
                g.side[0][j + 1],
            ];
            plane_face(sh, &edges, o + e * ra + vz * za, n)?;
        }
        // Top (outward z).
        let last = outline.len() - 1;
        let s_last = g.side[0].len() - 1;
        let edges = [g.cross[last], g.side[1][s_last], g.arc_t, g.side[0][s_last]];
        plane_face(sh, &edges, o + vz * rh, vz)?;
        // Shank panel over the gusset.
        shank_face(
            sh,
            &[g.arc_t, g.gen_[1], g.top_cap, g.gen_[0]],
            theta - ah - margin,
            theta + ah + margin,
        )?;
    }
    for k in 0..nr {
        let kn = (k + 1) % nr;
        let next = if k + 1 < nr { th[k + 1] } else { th[0] + tau };
        let edges = [
            base_arc[k],
            gs[kn].hyp[0],
            gs[kn].gen_[0],
            top_arc[k],
            gs[k].gen_[1],
            gs[k].hyp[1],
        ];
        shank_face(sh, &edges, th[k] + a0 - margin, next - a0 + margin)?;
    }
    // Shank top (outward z).
    let mut top = Vec::with_capacity(2 * nr);
    for k in 0..nr {
        top.push(gs[k].top_cap);
        top.push(top_arc[k]);
    }
    plane_face(sh, &top, o + vz * h, vz)?;
    // The footprint as an inner loop of the base (clockwise about z).
    let mut foot = Vec::new();
    for k in 0..nr {
        foot.push(gs[k].side[0][0]);
        foot.push(gs[k].cross[0]);
        foot.push(gs[k].side[1][0]);
        foot.push(base_arc[k]);
    }
    let mut wire = chain(sh, &foot)?;
    let (a, b) = basis(vz);
    if area(sh, &wire, Chart::Plane(o, a, b))? > 0.0 {
        wire = reversed(wire);
    }
    if !sh.faces[bf].orientation {
        wire = reversed(wire);
    }
    sh.faces[bf].boundaries.push(wire);
    let touched: BTreeSet<usize> = (body_edge_count(body)?..sh.edges.len()).collect();
    check_edges_on_faces(sh, &touched, tol)?;
    let out = rebuild(sh_)?;
    check_solid(&out)?;

    // Probes: inside the shank and a gusset; air between gussets and above
    // a gusset's top.
    let th0 = th[0];
    let e0 = vx * th0.cos() + vy * th0.sin();
    let mut probes = vec![
        o + vz * (0.5 * h),
        o + e0 * (0.5 * (s0 + outline.last().map_or(rl, |q| q.0))) + vz * (0.25 * rh),
        o + e0 * (0.5 * (r0 + rl)) + vz * (0.5 * (rh + h)),
    ];
    let next = if nr > 1 { th[1] } else { th0 + tau };
    let mid = 0.5 * (th0 + next);
    probes.push(polar(0.5 * (r0 + rl), mid, 0.5 * rh));
    let pts: Vec<[f64; 3]> = probes.iter().map(|p| p3(*p)).collect();
    let mm = crate::point_membership(&out, &pts)?;
    if mm != [Some(true), Some(true), Some(false), Some(false)] {
        return Err(KernelError::InvalidResult(format!(
            "gusseted boss failed the probe check ({mm:?})"
        )));
    }
    // Added volume: the frustum plus each gusset's cross-sections outside
    // the shank, A(z) = 2wL(z) − (w s(z) + R(z)² asin(w / R(z))), integrated
    // piecewise (Simpson) between the outline's heights.
    let reach_at = |z: f64| {
        for j in 0..outline.len() - 1 {
            let ((ra, za), (rb, zb)) = (outline[j], outline[j + 1]);
            if z >= za - 1e-12 && z <= zb + 1e-12 {
                return if zb - za < 1e-15 {
                    ra.max(rb)
                } else {
                    ra + (rb - ra) * (z - za) / (zb - za)
                };
            }
        }
        rl
    };
    let a_z = |z: f64| {
        let r = rad(z);
        2.0 * w * reach_at(z) - (w * s_at(z) + r * r * (w / r).asin())
    };
    let mut levels: Vec<f64> = outline.iter().map(|q| q.1).collect();
    levels.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut rib = 0.0;
    for pair in levels.windows(2) {
        let (za, zb) = (pair[0], pair[1]);
        let n = 400;
        let dz = (zb - za) / n as f64;
        let mut s = a_z(za) + a_z(zb);
        for i in 1..n {
            s += a_z(za + dz * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
        }
        rib += s * dz / 3.0;
    }
    let frustum = std::f64::consts::PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
    let want = frustum + nr as f64 * rib;
    let vol = |b: &Body| {
        crate::measure::verified_volume(b).map_err(|reason| {
            KernelError::InvalidResult(format!(
                "gusseted boss: cannot verify the added volume ({reason})"
            ))
        })
    };
    let ((v0, e0v), (v1, e1v)) = (vol(body)?, vol(&out)?);
    let tol_v = (1e-7 * v0.max(want)).max(1e-6) + 3.0 * (e0v + e1v);
    if ((v1 - v0) - want).abs() > tol_v {
        return Err(KernelError::InvalidResult(format!(
            "gusseted boss: added {:.9} mm³, expected {want:.9} mm³",
            v1 - v0
        )));
    }
    Ok(out)
}

fn body_edge_count(body: &Body) -> Result<usize> {
    Ok(shell_of(body)?.edges.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn plate() -> Body {
        let f = Frame {
            origin: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        crate::extrude(
            &crate::Profile::Polygon(vec![
                [-30.0, -20.0],
                [30.0, -20.0],
                [30.0, 20.0],
                [-30.0, 20.0],
            ]),
            &f,
            4.0,
        )
        .unwrap()
    }

    fn at(x: f64) -> Frame {
        Frame {
            origin: [x, 0.0, 4.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        }
    }

    fn spec(draft_deg: f64, n: usize) -> GussetBoss {
        GussetBoss {
            base_diameter: 6.0,
            height: 8.0,
            draft: draft_deg.to_radians(),
            angles: (0..n)
                .map(|k| PI / 4.0 + k as f64 * 2.0 * PI / n as f64)
                .collect(),
            thickness: 1.2,
            length: 7.0,
            chamfer: 3.0,
            rib_height: 6.0,
            tip_height: 2.0,
        }
    }

    fn vol(b: &Body) -> f64 {
        crate::measure::verified_volume(b).unwrap().0
    }

    /// One gusset's volume outside the shank, integrated across its
    /// thickness (y) from the side area at each y: independent of the
    /// kernel's integration in z. Shank radius `R(z) = r0 − z tan(a)`.
    fn rib_volume(r0: f64, a: f64, sp: &GussetBoss) -> f64 {
        let (w, l, ch, h0, h1) = (
            sp.thickness / 2.0,
            sp.length,
            sp.chamfer,
            sp.rib_height,
            sp.tip_height,
        );
        let t = a.tan();
        // Side area at offset y: the outline's area minus the part inside
        // the shank, ∫ over z in [0, h0] of sqrt(R(z)² − y²) dz.
        let outline = l * h0 - ch * (h0 - h1) / 2.0;
        let side = |y: f64| {
            let n = 2000;
            let dz = h0 / n as f64;
            let g = |z: f64| ((r0 - z * t).powi(2) - y * y).sqrt();
            let mut s = g(0.0) + g(h0);
            for i in 1..n {
                s += g(i as f64 * dz) * if i % 2 == 1 { 4.0 } else { 2.0 };
            }
            outline - s * dz / 3.0
        };
        let n = 2000;
        let dy = 2.0 * w / n as f64;
        let mut s = side(-w) + side(w);
        for i in 1..n {
            s += side(-w + i as f64 * dy) * if i % 2 == 1 { 4.0 } else { 2.0 };
        }
        s * dy / 3.0
    }

    #[test]
    fn drafted_boss_with_gussets_matches_the_integral() {
        let a = 2f64.to_radians();
        let sp = spec(2.0, 4);
        let out = gusset_boss(&plate(), &at(-15.0), &sp).unwrap();
        let r1 = 3.0 - 8.0 * a.tan();
        let frustum = PI * 8.0 * (9.0 + 3.0 * r1 + r1 * r1) / 3.0;
        let want = 9600.0 + frustum + 4.0 * rib_volume(3.0, a, &sp);
        assert!(
            (vol(&out) - want).abs() < 1e-6 * want,
            "{} vs {want}",
            vol(&out)
        );
        // A second boss with three gussets on the same plate.
        let sp3 = spec(2.0, 3);
        let out = gusset_boss(&out, &at(15.0), &sp3).unwrap();
        let want = want + frustum + 3.0 * rib_volume(3.0, a, &sp3);
        assert!(
            (vol(&out) - want).abs() < 1e-6 * want,
            "{} vs {want}",
            vol(&out)
        );
    }

    #[test]
    fn straight_shank_matches_the_integral() {
        let sp = spec(0.0, 4);
        let out = gusset_boss(&plate(), &at(0.0), &sp).unwrap();
        let want = 9600.0 + PI * 9.0 * 8.0 + 4.0 * rib_volume(3.0, 0.0, &sp);
        assert!(
            (vol(&out) - want).abs() < 1e-6 * want,
            "{} vs {want}",
            vol(&out)
        );
    }

    #[test]
    fn refusals_name_the_reason() {
        let p = plate();
        let mut s = spec(2.0, 12);
        s.thickness = 2.5;
        let e = gusset_boss(&p, &at(0.0), &s).unwrap_err().to_string();
        assert!(e.contains("meet on the shank"), "{e}");
        let e = gusset_boss(&p, &at(26.0), &spec(2.0, 4))
            .unwrap_err()
            .to_string();
        assert!(e.contains("does not fit"), "{e}");
        let mut s = spec(2.0, 4);
        s.rib_height = 8.0;
        let e = gusset_boss(&p, &at(0.0), &s).unwrap_err().to_string();
        assert!(e.contains("below the shank top"), "{e}");
    }
}
