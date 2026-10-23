//! Lip and groove (docs/PLASTICS.md "Lip"): a band along the rim of a
//! shelled part (a planar face with one outer and one inner boundary loop)
//! and the matching groove on the mating rim.
//!
//! * `lip`: on the rim face, the band `width` wide along the inner loop
//!   (`inner`) or the outer loop (`outer`), raised `height`;
//! * `groove`: the same band grown by `clearance` on both sides, cut
//!   `height + clearance` deep into the rim face;
//! * `lip_and_groove`: the lip on `face`, the groove on `mate` (the rim of
//!   the other half, the same footprint, facing the first).
//!
//! Bands are exact offsets of the rim's boundary (parallel lines,
//! concentric arcs, mitred corners). Fusion's edge-chain selection, face
//! guide, extents and trapezoidal sections are not modelled: the rim face
//! stands for the edge chain, and sections are rectangular.
use crate::util::{EPS, SINK, invalid, prism, unsupported};
use crate::{choice, input, param};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, FaceGeom, InputKind, Mode, Op, ParamKind, PluginError, Profile};

const F: &str = "lip";

pub fn schema() -> FeatureSchema {
    let pos = Some((0.0, true));
    FeatureSchema {
        id: F.into(),
        name: "Lip".into(),
        description: "Lip and/or groove along the rim of a shelled part".into(),
        inputs: vec![
            input("face", "Rim face", InputKind::Face, false),
            input(
                "mate",
                "Mating rim face (lip and groove)",
                InputKind::Face,
                true,
            ),
            input(
                "extents",
                "Extents (start and end on the rim)",
                InputKind::Points,
                true,
            ),
        ],
        params: vec![
            param(
                "width",
                "Width",
                ParamKind::Length,
                1.0,
                pos,
                None,
                Some("T/2"),
            ),
            param(
                "height",
                "Height",
                ParamKind::Length,
                2.0,
                pos,
                None,
                Some("RH"),
            ),
            param(
                "clearance",
                "Clearance",
                ParamKind::Length,
                0.1,
                Some((0.0, false)),
                None,
                Some("CL"),
            ),
            param(
                "draft_deg",
                "Draft (trapezoidal section)",
                ParamKind::Angle,
                0.0,
                Some((0.0, false)),
                Some(30.0),
                None,
            ),
        ],
        choices: vec![
            choice("type", "Type", &["lip", "groove", "lip_and_groove"]),
            choice("position", "Position", &["inner", "outer"]),
        ],
    }
}

/// (outer loop, inner loop) of a rim face.
fn rim(f: &FaceGeom, what: &str) -> Result<(Profile, Profile), PluginError> {
    match f.loops.as_slice() {
        [o, i] => Ok((o.clone(), i.clone())),
        [] => Err(unsupported(
            F,
            format!("the {what} face's boundary is not made of lines and arcs"),
        )),
        l => Err(invalid(
            F,
            format!(
                "the {what} face must be a rim (one outer and one inner boundary), it has {} loops",
                l.len()
            ),
        )),
    }
}

fn off(p: &Profile, d: f64, what: &str) -> Result<Profile, PluginError> {
    p.offset(d).map_err(|e| {
        invalid(
            F,
            format!("{what}: the offset by {d} mm does not fit ({e})"),
        )
    })
}

/// The band region (outer boundary, hole) `w` wide grown by `g` each side.
fn band(
    face: &FaceGeom,
    inner: bool,
    w: f64,
    g: f64,
    what: &str,
) -> Result<(Profile, Profile), PluginError> {
    let (o, i) = rim(face, what)?;
    let wall = o.area().map_err(|e| invalid(F, e))? - i.area().map_err(|e| invalid(F, e))?;
    let (outer, hole) = if inner {
        (off(&i, w + g, what)?, off(&i, -g, what)?)
    } else {
        (off(&o, g, what)?, off(&o, -(w + g), what)?)
    };
    // The band must stay within the wall: its own area below the wall's.
    let a = outer.area().map_err(|e| invalid(F, e))? - hole.area().map_err(|e| invalid(F, e))?;
    let own = if inner {
        off(&i, w, what)?.area().map_err(|e| invalid(F, e))?
            - i.area().map_err(|e| invalid(F, e))?
    } else {
        o.area().map_err(|e| invalid(F, e))?
            - off(&o, -w, what)?.area().map_err(|e| invalid(F, e))?
    };
    if own >= wall - EPS || a <= EPS {
        return Err(invalid(
            F,
            format!("the {what} band ({w} mm) is not narrower than the wall"),
        ));
    }
    Ok((outer, hole))
}

/// A piece of a rim loop (counter-clockwise): a line, or an arc about
/// `c` (`ccw`: turning with the loop, i.e. convex).
#[derive(Clone, Copy)]
enum Piece {
    Line([f64; 2], [f64; 2]),
    Arc {
        c: [f64; 2],
        p: [f64; 2],
        q: [f64; 2],
        ccw: bool,
    },
}

fn pieces_of(p: &Profile) -> Result<Vec<Piece>, PluginError> {
    use rcad_plugin::Segment;
    let canon = Profile::from_sketch(&p.to_sketch().map_err(|e| invalid(F, e))?)
        .ok_or_else(|| unsupported(F, "the rim has free-form edges"))?;
    Ok(match canon {
        Profile::Circle { center, radius } => {
            let a = [center[0] + radius, center[1]];
            let b = [center[0] - radius, center[1]];
            vec![
                Piece::Arc {
                    c: center,
                    p: a,
                    q: b,
                    ccw: true,
                },
                Piece::Arc {
                    c: center,
                    p: b,
                    q: a,
                    ccw: true,
                },
            ]
        }
        Profile::Polygon { points } => (0..points.len())
            .map(|i| Piece::Line(points[i], points[(i + 1) % points.len()]))
            .collect(),
        Profile::Loop { segments } => (0..segments.len())
            .map(|i| {
                let q = segments[(i + 1) % segments.len()].start();
                match segments[i] {
                    Segment::Line { start } => Piece::Line(start, q),
                    Segment::Arc { start, center, ccw } => Piece::Arc {
                        c: center,
                        p: start,
                        q,
                        ccw,
                    },
                }
            })
            .collect(),
    })
}

fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

impl Piece {
    fn start(&self) -> [f64; 2] {
        match *self {
            Piece::Line(p, _) | Piece::Arc { p, .. } => p,
        }
    }
    fn end(&self) -> [f64; 2] {
        match *self {
            Piece::Line(_, q) | Piece::Arc { q, .. } => q,
        }
    }
    /// Nearest point of the piece to `x` and its distance.
    fn project(&self, x: [f64; 2]) -> ([f64; 2], f64) {
        match *self {
            Piece::Line(p, q) => {
                let d = sub2(q, p);
                let t = ((x[0] - p[0]) * d[0] + (x[1] - p[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]);
                let t = t.clamp(0.0, 1.0);
                let y = [p[0] + d[0] * t, p[1] + d[1] * t];
                (y, (x[0] - y[0]).hypot(x[1] - y[1]))
            }
            Piece::Arc { c, p, q, ccw } => {
                let r = (p[0] - c[0]).hypot(p[1] - c[1]);
                let ang = |v: [f64; 2]| (v[1] - c[1]).atan2(v[0] - c[0]);
                let tau = std::f64::consts::TAU;
                let (a0, a1, ax) = (ang(p), ang(q), ang(x));
                let span = if ccw {
                    (a1 - a0).rem_euclid(tau)
                } else {
                    (a0 - a1).rem_euclid(tau)
                };
                let off = if ccw {
                    (ax - a0).rem_euclid(tau)
                } else {
                    (a0 - ax).rem_euclid(tau)
                };
                let y = if off <= span {
                    [c[0] + r * ax.cos(), c[1] + r * ax.sin()]
                } else if (off - span) < (tau - off) {
                    q
                } else {
                    p
                };
                (y, (x[0] - y[0]).hypot(x[1] - y[1]))
            }
        }
    }
    /// The piece from `a` to `b` (both on it, in its direction).
    fn between(&self, a: [f64; 2], b: [f64; 2]) -> Piece {
        match *self {
            Piece::Line(..) => Piece::Line(a, b),
            Piece::Arc { c, ccw, .. } => Piece::Arc { c, p: a, q: b, ccw },
        }
    }
    /// Point `x` of the piece moved `d` outward (right of a CCW loop).
    fn offset(&self, x: [f64; 2], d: f64) -> [f64; 2] {
        match *self {
            Piece::Line(p, q) => {
                let v = sub2(q, p);
                let l = v[0].hypot(v[1]);
                [x[0] + d * v[1] / l, x[1] - d * v[0] / l]
            }
            Piece::Arc { c, ccw, .. } => {
                let v = sub2(x, c);
                let r = v[0].hypot(v[1]);
                let k = (r + if ccw { d } else { -d }) / r;
                [c[0] + v[0] * k, c[1] + v[1] * k]
            }
        }
    }
}

/// The band between offsets `d0 < d1` (outward of the counter-clockwise
/// rim loop `lp`) along the loop from the point nearest `from` to the point
/// nearest `to`, as one simple region: exact offset lines and concentric
/// arcs, mitred at sharp line corners, square at its ends (Fusion's lip
/// extents).
fn chain_band(
    lp: &Profile,
    from: [f64; 2],
    to: [f64; 2],
    d0: f64,
    d1: f64,
) -> Result<Profile, PluginError> {
    use rcad_plugin::Segment;
    let ps = pieces_of(lp)?;
    let near = |x: [f64; 2]| {
        ps.iter()
            .enumerate()
            .map(|(i, p)| (i, p.project(x)))
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map(|(i, (y, _))| (i, y))
    };
    let ((i0, q0), (i1, q1)) = (
        near(from).ok_or_else(|| invalid(F, "empty rim"))?,
        near(to).ok_or_else(|| invalid(F, "empty rim"))?,
    );
    if (q0[0] - q1[0]).hypot(q0[1] - q1[1]) < EPS {
        return Err(invalid(F, "the two extents are at one point of the rim"));
    }
    // The chain, start to end, along the loop.
    let n = ps.len();
    let mut chain = Vec::new();
    if i0 == i1 && {
        // The end lies after the start on the same piece.
        let a = sub2(q0, ps[i0].start());
        let b = sub2(q1, ps[i0].start());
        a[0].hypot(a[1]) <= b[0].hypot(b[1])
    } {
        chain.push(ps[i0].between(q0, q1));
    } else {
        chain.push(ps[i0].between(q0, ps[i0].end()));
        let mut k = (i0 + 1) % n;
        while k != i1 {
            chain.push(ps[k]);
            k = (k + 1) % n;
        }
        chain.push(ps[i1].between(ps[i1].start(), q1));
    }
    chain.retain(|p| {
        let v = sub2(p.end(), p.start());
        v[0].hypot(v[1]) > EPS
    });
    // Offset path vertices: piece starts (mitred between sharp lines) and the end.
    let path = |d: f64| -> Vec<[f64; 2]> {
        let mut v = Vec::new();
        for (k, p) in chain.iter().enumerate() {
            let s = p.start();
            let here = p.offset(s, d);
            let pt = match (k.checked_sub(1).map(|j| chain[j]), p) {
                (Some(Piece::Line(a, b)), Piece::Line(c, e)) => {
                    // Intersection of the two offset lines.
                    let (o1, o2) = (Piece::Line(a, b).offset(b, d), here);
                    let (u, w) = (sub2(b, a), sub2(*e, *c));
                    let den = u[0] * w[1] - u[1] * w[0];
                    if den.abs() < 1e-12 {
                        here
                    } else {
                        let t = ((o2[0] - o1[0]) * w[1] - (o2[1] - o1[1]) * w[0]) / den;
                        [o1[0] + u[0] * t, o1[1] + u[1] * t]
                    }
                }
                _ => here,
            };
            v.push(pt);
        }
        let last = chain[chain.len() - 1];
        v.push(last.offset(last.end(), d));
        v
    };
    let (inner, outer) = (path(d0), path(d1));
    let mut segs = Vec::new();
    for (k, p) in chain.iter().enumerate() {
        segs.push(match p {
            Piece::Line(..) => Segment::Line { start: inner[k] },
            Piece::Arc { c, ccw, .. } => Segment::Arc {
                start: inner[k],
                center: *c,
                ccw: *ccw,
            },
        });
    }
    segs.push(Segment::Line {
        start: inner[chain.len()],
    });
    for k in (0..chain.len()).rev() {
        segs.push(match chain[k] {
            Piece::Line(..) => Segment::Line {
                start: outer[k + 1],
            },
            Piece::Arc { c, ccw, .. } => Segment::Arc {
                start: outer[k + 1],
                center: c,
                ccw: !ccw,
            },
        });
    }
    segs.push(Segment::Line { start: outer[0] });
    Ok(Profile::Loop { segments: segs })
}

/// Planes (point, unit normal) of the faces standing on the edges of the
/// straight-edged loop `p` (in `f`'s plane), normals to the right of the
/// counter-clockwise loop (`right`) or to its left.
fn edge_planes(f: &FaceGeom, p: &Profile, right: bool) -> Result<Vec<[[f64; 3]; 2]>, PluginError> {
    let canon = Profile::from_sketch(&p.to_sketch().map_err(|e| invalid(F, e))?);
    let pts = match canon {
        Some(Profile::Polygon { points }) => points,
        _ => {
            return Err(unsupported(
                F,
                "drafted lips need a rim of straight edges (no arcs)",
            ));
        }
    };
    let n = pts.len();
    Ok((0..n)
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % n]);
            let l = (b[0] - a[0]).hypot(b[1] - a[1]);
            let (ex, ey) = ((b[0] - a[0]) / l, (b[1] - a[1]) / l);
            let (nx, ny) = if right { (ey, -ex) } else { (-ey, ex) };
            let nrm = rcad_plugin::geom::add(
                rcad_plugin::geom::scale(f.frame.x, nx),
                rcad_plugin::geom::scale(f.frame.y, ny),
            );
            [f.frame.point(a, 0.0), nrm]
        })
        .collect())
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let (w, h, cl) = (
        req.param("width")?,
        req.param("height")?,
        req.param("clearance")?,
    );
    let inner = req.choice("position")? == "inner";
    let face = req.face("face")?;
    let kind = req.choice("type")?;
    // Extents: two points on the lip's face; a partial band between them.
    let ext: Option<[[f64; 3]; 2]> = if req.has("extents") {
        let p = req.points("extents")?;
        let [a, b] = p else {
            return Err(invalid(
                F,
                "extents are two points (start and end along the rim)",
            ));
        };
        Some([face.frame.point(*a, 0.0), face.frame.point(*b, 0.0)])
    } else {
        None
    };
    // The band on face `f` between outward offsets (d0, d1) of the rim
    // loop: whole (with a hole) or between the extents.
    let shape =
        |f: &FaceGeom, g: f64, what: &str| -> Result<(Profile, Vec<Profile>), PluginError> {
            let (o, i) = rim(f, what)?;
            band(f, inner, w, g, what)?;
            match ext {
                None => {
                    let (outer, hole) = band(f, inner, w, g, what)?;
                    Ok((outer, vec![hole]))
                }
                Some([a, b]) => {
                    let (a2, b2) = (f.frame.local(a).0, f.frame.local(b).0);
                    let (lp, d0, d1) = if inner {
                        (i, -g, w + g)
                    } else {
                        (o, -(w + g), g)
                    };
                    // The mate's frame is mirrored: its loop runs the other way,
                    // so the extents swap to cover the same stretch.
                    let flip = rcad_plugin::geom::dot(f.frame.z, face.frame.z) < 0.0;
                    let (s, e) = if flip { (b2, a2) } else { (a2, b2) };
                    Ok((chain_band(&lp, s, e, d0, d1)?, vec![]))
                }
            }
        };
    let mut ops = Vec::new();
    let groove = |input: &str, f: &FaceGeom, id: u32| -> Result<Op, PluginError> {
        let (outer, hole) = shape(f, cl, "groove")?;
        if h + cl >= -f.body_span[0] - EPS {
            return Err(invalid(F, "the groove is deeper than the part"));
        }
        Ok(prism(
            id,
            input,
            f.frame,
            outer,
            hole,
            (-(h + cl), 1.0),
            0.0,
            Mode::Cut,
            "groove".into(),
        ))
    };
    if kind != "groove" {
        let (outer, hole) = shape(face, 0.0, "lip")?;
        ops.push(prism(
            0,
            "face",
            face.frame,
            outer,
            hole,
            // Sunk into the rim (like every plastics tool standing on a
            // face), so the band is joined by the general checked boolean
            // also when the body is no longer prismatic under the rim.
            (-SINK, h),
            0.0,
            Mode::Join,
            "lip".into(),
        ));
    }
    // Draft (trapezoidal section): the lip's reveal faces lean in going up
    // and the groove's matching wall narrows with depth, both turned about
    // their rim planes (exact polyhedral draft).
    let da = req.param("draft_deg")?;
    let draft = |f: &FaceGeom,
                 input: &str,
                 id: u32,
                 d: f64,
                 wall_right: bool,
                 what: &str|
     -> Result<Op, PluginError> {
        if ext.is_some() {
            return Err(unsupported(
                F,
                "drafted lips run along the whole rim (no extents)",
            ));
        }
        let (o, i) = rim(f, what)?;
        let lp = if inner {
            off(&i, d, what)?
        } else {
            off(&o, -d, what)?
        };
        Ok(Op::DraftFaces {
            id,
            body: input.into(),
            faces: edge_planes(f, &lp, wall_right)?,
            plane_point: f.frame.origin,
            plane_normal: f.frame.z,
            pull: f.frame.z,
            angle_deg: da,
            two_sided: false,
            label: format!("{what} draft"),
        })
    };
    if da > 0.0 && kind != "groove" {
        if w - h * da.to_radians().tan() <= EPS {
            return Err(invalid(F, "the draft closes the lip before its height"));
        }
        // The lip's reveal faces face away from the wall's middle.
        ops.push(draft(face, "face", 2, w, inner, "lip")?);
    }
    match kind {
        "groove" => ops.push(groove("face", face, 1)?),
        "lip_and_groove" => {
            let m = req
                .face("mate")
                .map_err(|_| invalid(F, "lip and groove needs the mating rim face"))?;
            if (rcad_plugin::geom::dot(m.frame.z, face.frame.z) + 1.0).abs() > 1e-9 {
                return Err(invalid(
                    F,
                    "the mating rim must face the rim (parallel, opposite normals)",
                ));
            }
            ops.push(groove("mate", m, 1)?);
        }
        _ => {}
    }
    if da > 0.0 && kind != "lip" {
        let (f, input) = if kind == "groove" {
            (face, "face")
        } else {
            (req.face("mate")?, "mate")
        };
        // The groove's outer wall faces into the groove (towards the cavity
        // for an inner lip); turned about the mouth it narrows with depth.
        ops.push(draft(f, input, 3, w + cl, !inner, "groove")?);
    }
    Ok(ops)
}
