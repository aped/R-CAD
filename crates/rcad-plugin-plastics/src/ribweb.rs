//! Rib and Web (docs/PLASTICS.md "Rib and Web"), from open chains of sketch
//! lines, each line one operation (id = its position among the sketch's
//! lines):
//!
//! * **Rib**: thickness across the sketch plane (`symmetric` about it, or
//!   `one_side` on its normal side), material grown in the plane from the
//!   line by `depth` to its `left`/`right` (left: sketch normal × line
//!   direction), or `to_next`: to the nearest planar face of the body in
//!   that direction, along the whole line (sunk 0.2 mm into it). Draft
//!   narrows the rib towards its far edge.
//! * **Web**: thickness in the sketch plane across the line, material grown
//!   along the sketch normal (`flip`: against it) by `depth`, or `to_next`:
//!   to the nearest face of the sketch's body facing it (sunk 0.2 mm into
//!   that face). Draft narrows the web away from the sketch plane.
//!
//! Lines meeting at an end are each extended by half the thickness there,
//! so corners are filled. Arcs are exact annular-sector prisms (not drafted;
//! a drafted arc wall would be a cone section); circles are refused. Not supported: fillets, extending free
//! ends to the next face (Fusion's Extend Curves).
use crate::util::SINK;
use crate::util::{EPS, invalid, prism, unsupported};
use crate::{choice, input, param};
use rcad_plugin::geom::{add, cross, footprint_of, next_above_over, next_below_over, scale};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{
    BuildRequest, Curve, Frame, InputKind, Mode, Op, ParamKind, PluginError, Profile, Segment,
    SketchGeom,
};

fn common(
    id: &str,
    name: &str,
    description: &str,
    extra: Vec<rcad_plugin::ChoiceSchema>,
) -> FeatureSchema {
    let pos = Some((0.0, true));
    let mut choices = vec![choice(
        "thickness_side",
        "Thickness",
        &["symmetric", "one_side"],
    )];
    choices.extend(extra);
    FeatureSchema {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        inputs: vec![
            input("sketch", "Sketch lines", InputKind::Sketch, false),
            input("body", "Body (sketch not on a face)", InputKind::Body, true),
        ],
        params: vec![
            param(
                "thickness",
                "Thickness",
                ParamKind::Length,
                2.0,
                pos,
                None,
                Some("T"),
            ),
            param("depth", "Depth", ParamKind::Length, 10.0, pos, None, None),
            param(
                "draft_deg",
                "Draft",
                ParamKind::Angle,
                0.0,
                Some((0.0, false)),
                Some(30.0),
                Some("DA"),
            ),
        ],
        choices,
    }
}

pub fn rib_schema() -> FeatureSchema {
    common(
        "rib",
        "Rib",
        "Thin wall grown in the sketch plane from open sketch lines",
        vec![
            choice("side", "Direction", &["left", "right"]),
            choice("extent", "Extent", &["distance", "to_next"]),
        ],
    )
}

pub fn web_schema() -> FeatureSchema {
    common(
        "web",
        "Web",
        "Network of thin walls grown along the sketch normal from open sketch lines",
        vec![
            choice("direction", "Direction", &["normal", "flip"]),
            choice("extent", "Extent", &["distance", "to_next"]),
        ],
    )
}

/// A line `(a, b)` and how far to extend it at `a` and at `b`.
type Ends = ([f64; 2], [f64; 2], f64, f64);

/// A sketch arc: centre, radius, start angle and counter-clockwise sweep.
#[derive(Clone, Copy)]
struct Arc {
    c: [f64; 2],
    r: f64,
    a0: f64,
    sweep: f64,
}

impl Arc {
    fn at(&self, f: f64) -> [f64; 2] {
        let t = self.a0 + self.sweep * f;
        [self.c[0] + self.r * t.cos(), self.c[1] + self.r * t.sin()]
    }

    /// The annular sector between radii `r0 < r1` over the arc's angles.
    fn sector(&self, f: &str, r0: f64, r1: f64) -> Result<Profile, PluginError> {
        if r0 <= EPS {
            return Err(invalid(f, "the wall reaches the centre of a sketch arc"));
        }
        let p = |r: f64, t: f64| {
            let a = self.a0 + self.sweep * t;
            [self.c[0] + r * a.cos(), self.c[1] + r * a.sin()]
        };
        Ok(Profile::Loop {
            segments: vec![
                Segment::Arc {
                    start: p(r1, 0.0),
                    center: self.c,
                    ccw: true,
                },
                Segment::Line { start: p(r1, 1.0) },
                Segment::Arc {
                    start: p(r0, 1.0),
                    center: self.c,
                    ccw: false,
                },
                Segment::Line { start: p(r0, 0.0) },
            ],
        })
    }
}

/// The sketch's lines (with how far to extend each end: half the
/// thickness where another curve ends at the same point) and arcs, each with
/// its index among the sketch's curves (the operation id). Circles are
/// refused (a closed ring is a thin extrusion, not a rib).
fn curves(
    f: &str,
    s: &SketchGeom,
    t: f64,
) -> Result<(Vec<(u32, Ends)>, Vec<(u32, Arc)>), PluginError> {
    let mut lines = Vec::new();
    let mut arcs = Vec::new();
    let mut ends: Vec<(usize, [f64; 2])> = Vec::new();
    for (i, c) in s.curves.iter().enumerate() {
        match *c {
            Curve::Line { a, b } => {
                lines.push((i, a, b));
                ends.push((i, a));
                ends.push((i, b));
            }
            Curve::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                if sweep < 1e-9 {
                    return Err(invalid(f, "a sketch arc has no sweep"));
                }
                let arc = Arc {
                    c: center,
                    r: radius,
                    a0: start,
                    sweep,
                };
                ends.push((i, arc.at(0.0)));
                ends.push((i, arc.at(1.0)));
                arcs.push((i as u32, arc));
            }
            Curve::Circle { .. } => {
                return Err(unsupported(
                    f,
                    "circles are not supported (lines and arcs are; a closed ring is a thin extrusion)",
                ));
            }
        }
    }
    if lines.is_empty() && arcs.is_empty() {
        return Err(invalid(f, "the sketch has no lines or arcs"));
    }
    let same = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < EPS;
    let shared = |i: usize, p: [f64; 2]| ends.iter().any(|(j, q)| *j != i && same(*q, p));
    let lines = lines
        .into_iter()
        .map(|(i, a, b)| {
            let ea = if shared(i, a) { t / 2.0 } else { 0.0 };
            let eb = if shared(i, b) { t / 2.0 } else { 0.0 };
            (i as u32, (a, b, ea, eb))
        })
        .collect();
    Ok((lines, arcs))
}

fn target(req: &BuildRequest, f: &str) -> Result<&'static str, PluginError> {
    if req.has("body") {
        Ok("body")
    } else if req.sketch("sketch")?.body.is_some() {
        Ok("sketch")
    } else {
        Err(invalid(
            f,
            "the sketch is not on a face of a body; choose the body",
        ))
    }
}

struct Line {
    a: [f64; 2],
    u: [f64; 2],
    len: f64,
    ea: f64,
    eb: f64,
}

fn line(f: &str, a: [f64; 2], b: [f64; 2], ea: f64, eb: f64) -> Result<Line, PluginError> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = d[0].hypot(d[1]);
    if len < EPS {
        return Err(invalid(f, "a sketch line has no length"));
    }
    Ok(Line {
        a,
        u: [d[0] / len, d[1] / len],
        len,
        ea,
        eb,
    })
}

impl Line {
    /// Point at distance `s` along the line and `w` to its left.
    fn at(&self, s: f64, w: f64) -> [f64; 2] {
        [
            self.a[0] + self.u[0] * s - self.u[1] * w,
            self.a[1] + self.u[1] * s + self.u[0] * w,
        ]
    }

    /// Rectangle from `s0..s1` along and `w0..w1` to the left.
    fn rect(&self, (w0, w1): (f64, f64)) -> Profile {
        let (s0, s1) = (-self.ea, self.len + self.eb);
        Profile::Polygon {
            points: vec![
                self.at(s0, w0),
                self.at(s1, w0),
                self.at(s1, w1),
                self.at(s0, w1),
            ],
        }
    }

    /// Frame standing on the line: `x` = `xdir` (world), `y` = `ydir`,
    /// `z = x × y` (±the line direction); returns the frame and the span
    /// along `z` covering the (extended) line.
    fn standing(&self, s: &Frame, xdir: [f64; 3], ydir: [f64; 3]) -> (Frame, (f64, f64)) {
        let z = cross(xdir, ydir);
        let along = add(scale(s.x, self.u[0]), scale(s.y, self.u[1]));
        let sign = rcad_plugin::geom::dot(z, along).signum();
        let span = if sign > 0.0 {
            (-self.ea, self.len + self.eb)
        } else {
            (-self.len - self.eb, self.ea)
        };
        (
            Frame {
                origin: s.point(self.a, 0.0),
                x: xdir,
                y: ydir,
                z,
            },
            span,
        )
    }

    /// In-plane left normal (world).
    fn left(&self, s: &Frame) -> [f64; 3] {
        add(scale(s.x, -self.u[1]), scale(s.y, self.u[0]))
    }
}

/// A trapezoid section: base `w` wide at `x = 0` across `[c - w/2, c + w/2]`
/// (or `[0, w]` one-sided), narrowing by `tan(a)` per mm on each drafted
/// side up to `x = h`.
fn section(f: &str, w: f64, h: f64, a: f64, symmetric: bool) -> Result<Profile, PluginError> {
    let k = h * a.tan();
    let (lo, hi, klo) = if symmetric {
        (-w / 2.0, w / 2.0, k)
    } else {
        (0.0, w, 0.0)
    };
    let top = (hi - k) - (lo + klo);
    if top <= EPS {
        return Err(invalid(
            f,
            format!("the draft closes the wall before its depth ({top:.4} mm left)"),
        ));
    }
    Ok(Profile::Polygon {
        points: vec![[0.0, lo], [h, lo + klo], [h, hi - k], [0.0, hi]],
    })
}

pub fn build_rib(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let f = "rib";
    let s = req.sketch("sketch")?;
    let body = target(req, f)?;
    let (t, depth, a) = (
        req.param("thickness")?,
        req.param("depth")?,
        req.param("draft_deg")?.to_radians(),
    );
    let sym = req.choice("thickness_side")? == "symmetric";
    let left = req.choice("side")? == "left";
    let to_next = req.choice("extent")? == "to_next";
    let faces: &[rcad_plugin::PlanarFace] = if req.has("body") {
        &req.body("body")?.faces
    } else {
        &s.faces
    };
    let mut ops = Vec::new();
    let (lines, arcs) = curves(f, s, t)?;
    for (id, (pa, pb, ea, eb)) in lines {
        let i = id as usize;
        let l = line(f, pa, pb, ea, eb)?;
        let label = format!("rib line {}", i + 1);
        let depth = if to_next {
            // Rays from points along the line in the growth direction to the
            // nearest planar face facing them; the farthest hit, so the rib
            // reaches that face along its whole length, sunk into it.
            let dir = if left {
                l.left(&s.frame)
            } else {
                scale(l.left(&s.frame), -1.0)
            };
            let (mut near, mut far) = (f64::INFINITY, 0.0f64);
            // Interior samples: an end on a wall would hit that wall's edge.
            for k in 0..8 {
                let p = s
                    .frame
                    .point(l.at(l.len * (k as f64 + 0.5) / 8.0, 0.0), 0.0);
                let hit = rcad_plugin::geom::ray_to_face(faces, p, dir, EPS).ok_or_else(|| {
                    invalid(
                        f,
                        format!(
                            "to next: no planar face of the body in the rib's direction from rib line {}",
                            i + 1
                        ),
                    )
                })?;
                near = near.min(hit);
                far = far.max(hit);
            }
            // A constant-depth rib reaches the face exactly only where it is
            // at one distance along the whole curve.
            if far - near > 1e-6 {
                return Err(unsupported(
                    f,
                    format!(
                        "to next: the next face is {near:.3} to {far:.3} mm away along the curve; \
                         a rib has one depth (use a distance)"
                    ),
                ));
            }
            far + SINK
        } else {
            depth
        };
        let (w0, w1) = if left { (0.0, depth) } else { (-depth, 0.0) };
        if a == 0.0 {
            let span = if sym { (-t / 2.0, t / 2.0) } else { (0.0, t) };
            ops.push(prism(
                i as u32,
                body,
                s.frame,
                l.rect((w0, w1)),
                vec![],
                span,
                0.0,
                Mode::Join,
                label,
            ));
        } else {
            let dir = if left {
                l.left(&s.frame)
            } else {
                scale(l.left(&s.frame), -1.0)
            };
            let (fr, span) = l.standing(&s.frame, dir, s.frame.z);
            let sec = section(f, t, depth, a, sym)?;
            ops.push(prism(
                i as u32,
                body,
                fr,
                sec,
                vec![],
                span,
                0.0,
                Mode::Join,
                label,
            ));
        }
    }
    for (id, arc) in arcs {
        let label = format!("rib arc {}", id + 1);
        if a != 0.0 {
            return Err(unsupported(
                f,
                "drafted ribs along arcs are not supported (straight lines only)",
            ));
        }
        // Left of a counter-clockwise arc is towards its centre.
        let d = if to_next {
            let (mut near, mut far) = (f64::INFINITY, 0.0f64);
            for k in 0..8 {
                let q = arc.at((k as f64 + 0.5) / 8.0);
                let out = [q[0] - arc.c[0], q[1] - arc.c[1]];
                let n = out[0].hypot(out[1]);
                let u = if left {
                    [-out[0] / n, -out[1] / n]
                } else {
                    [out[0] / n, out[1] / n]
                };
                let dir = add(scale(s.frame.x, u[0]), scale(s.frame.y, u[1]));
                let p = s.frame.point(q, 0.0);
                let hit = rcad_plugin::geom::ray_to_face(faces, p, dir, EPS).ok_or_else(|| {
                    invalid(f, format!("to next: no planar face of the body in the rib's direction from rib arc {}", id + 1))
                })?;
                near = near.min(hit);
                far = far.max(hit);
            }
            // A constant-depth rib reaches the face exactly only where it is
            // at one distance along the whole curve.
            if far - near > 1e-6 {
                return Err(unsupported(
                    f,
                    format!(
                        "to next: the next face is {near:.3} to {far:.3} mm away along the curve; \
                         a rib has one depth (use a distance)"
                    ),
                ));
            }
            far + SINK
        } else {
            depth
        };
        let (r0, r1) = if left {
            (arc.r - d, arc.r)
        } else {
            (arc.r, arc.r + d)
        };
        let span = if sym { (-t / 2.0, t / 2.0) } else { (0.0, t) };
        ops.push(prism(
            id,
            body,
            s.frame,
            arc.sector(f, r0, r1)?,
            vec![],
            span,
            0.0,
            Mode::Join,
            label,
        ));
    }
    Ok(ops)
}

pub fn build_web(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let f = "web";
    let s = req.sketch("sketch")?;
    let body = target(req, f)?;
    let (t, a) = (
        req.param("thickness")?,
        req.param("draft_deg")?.to_radians(),
    );
    let sym = req.choice("thickness_side")? == "symmetric";
    let up = req.choice("direction")? == "normal";
    let to_next = req.choice("extent")? == "to_next";
    let mut ops = Vec::new();
    let (lines, arcs) = curves(f, s, t)?;
    for (id, (pa, pb, ea, eb)) in lines {
        let i = id as usize;
        let l = line(f, pa, pb, ea, eb)?;
        let label = format!("web line {}", i + 1);
        let (w0, w1) = if sym { (-t / 2.0, t / 2.0) } else { (0.0, t) };
        let depth = if to_next {
            // The nearest face facing the sketch plane under this line,
            // sunk into it so the web's end is not coplanar with it.
            let fp = footprint_of(&[pa, pb], t);
            let next = if up {
                next_above_over(&s.parallel, 0.0, EPS, &fp)
            } else {
                next_below_over(&s.parallel, 0.0, EPS, &fp).map(|h| -h)
            };
            next.ok_or_else(|| {
                invalid(
                    f,
                    format!("to next: no face of the sketch's body under web line {} faces the sketch plane", i + 1),
                )
            })? + SINK
        } else {
            req.param("depth")?
        };
        if a == 0.0 {
            let span = if up { (0.0, depth) } else { (-depth, 0.0) };
            ops.push(prism(
                i as u32,
                body,
                s.frame,
                l.rect((w0, w1)),
                vec![],
                span,
                0.0,
                Mode::Join,
                label,
            ));
        } else {
            // Section in (depth along ±normal, thickness across the line).
            let n = if up {
                s.frame.z
            } else {
                scale(s.frame.z, -1.0)
            };
            let (fr, span) = l.standing(&s.frame, n, l.left(&s.frame));
            let sec = section(f, t, depth, a, sym)?;
            ops.push(prism(
                i as u32,
                body,
                fr,
                sec,
                vec![],
                span,
                0.0,
                Mode::Join,
                label,
            ));
        }
    }
    for (id, arc) in arcs {
        let label = format!("web arc {}", id + 1);
        if a != 0.0 {
            return Err(unsupported(
                f,
                "drafted webs along arcs are not supported (straight lines only)",
            ));
        }
        let depth = if to_next {
            let fp = footprint_of(&[arc.c], arc.r + t);
            let next = if up {
                next_above_over(&s.parallel, 0.0, EPS, &fp)
            } else {
                next_below_over(&s.parallel, 0.0, EPS, &fp).map(|h| -h)
            };
            next.ok_or_else(|| {
                invalid(f, format!("to next: no face of the sketch's body under web arc {} faces the sketch plane", id + 1))
            })? + SINK
        } else {
            req.param("depth")?
        };
        // Symmetric about the arc, or on its left (towards the centre).
        let (r0, r1) = if sym {
            (arc.r - t / 2.0, arc.r + t / 2.0)
        } else {
            (arc.r - t, arc.r)
        };
        let span = if up { (0.0, depth) } else { (-depth, 0.0) };
        ops.push(prism(
            id,
            body,
            s.frame,
            arc.sector(f, r0, r1)?,
            vec![],
            span,
            0.0,
            Mode::Join,
            label,
        ));
    }
    Ok(ops)
}
