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
//! so corners are filled. Arcs and circles are refused (a straight prism per
//! curve is exact only for lines). Not supported: fillets, extending free
//! ends to the next face (Fusion's Extend Curves).
use crate::util::SINK;
use crate::util::{EPS, invalid, prism, unsupported};
use crate::{choice, input, param};
use rcad_plugin::geom::{add, cross, footprint_of, next_above_over, next_below_over, scale};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{
    BuildRequest, Curve, Frame, InputKind, Mode, Op, ParamKind, PluginError, Profile, SketchGeom,
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

/// The sketch's lines with how far to extend each end (half the thickness
/// where another line ends at the same point).
fn lines(f: &str, s: &SketchGeom, t: f64) -> Result<Vec<Ends>, PluginError> {
    let mut v = Vec::new();
    for c in &s.curves {
        match c {
            Curve::Line { a, b } => v.push((*a, *b)),
            _ => {
                return Err(unsupported(
                    f,
                    "only straight sketch lines are supported (arcs and circles are not)",
                ));
            }
        }
    }
    if v.is_empty() {
        return Err(invalid(f, "the sketch has no lines"));
    }
    let same = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < EPS;
    let shared = |i: usize, p: [f64; 2]| {
        v.iter()
            .enumerate()
            .any(|(j, (a, b))| j != i && (same(*a, p) || same(*b, p)))
    };
    Ok(v.iter()
        .enumerate()
        .map(|(i, (a, b))| {
            let ea = if shared(i, *a) { t / 2.0 } else { 0.0 };
            let eb = if shared(i, *b) { t / 2.0 } else { 0.0 };
            (*a, *b, ea, eb)
        })
        .collect())
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
    for (i, (pa, pb, ea, eb)) in lines(f, s, t)?.into_iter().enumerate() {
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
            let mut far: f64 = 0.0;
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
                far = far.max(hit);
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
    for (i, (pa, pb, ea, eb)) in lines(f, s, t)?.into_iter().enumerate() {
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
    Ok(ops)
}
