//! Geometry exchanged with plugins: frames, plane profiles, and the resolved
//! inputs a build receives. Plain serializable data, so a plugin in another
//! process sees exactly what an in-process plugin sees.
use rcad_sketch::{LoopSegment, Profile as SketchProfile};
use serde::{Deserialize, Serialize};

/// Right-handed orthonormal frame (world mm): plane coordinates `(u, v)` map
/// to `origin + u·x + v·y`; `z` is the plane normal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

impl Frame {
    /// The world point at plane coordinates `p` and height `h` along `z`.
    pub fn point(&self, p: [f64; 2], h: f64) -> [f64; 3] {
        add(
            add(self.origin, add(scale(self.x, p[0]), scale(self.y, p[1]))),
            scale(self.z, h),
        )
    }

    /// Plane coordinates and height of world point `q`.
    pub fn local(&self, q: [f64; 3]) -> ([f64; 2], f64) {
        let d = sub(q, self.origin);
        ([dot(d, self.x), dot(d, self.y)], dot(d, self.z))
    }

    /// This frame moved `h` along its normal.
    pub fn offset(&self, h: f64) -> Frame {
        Frame {
            origin: add(self.origin, scale(self.z, h)),
            ..*self
        }
    }

    /// This frame moved to plane point `p` (axes unchanged).
    pub fn at(&self, p: [f64; 2]) -> Frame {
        Frame {
            origin: self.point(p, 0.0),
            ..*self
        }
    }

    /// The frame with its normal reversed (x kept, y flipped: right-handed).
    pub fn flipped(&self) -> Frame {
        Frame {
            origin: self.origin,
            x: self.x,
            y: scale(self.y, -1.0),
            z: scale(self.z, -1.0),
        }
    }

    /// The frame rotated by `a` radians about its own normal.
    pub fn rotated(&self, a: f64) -> Frame {
        let (s, c) = a.sin_cos();
        Frame {
            origin: self.origin,
            x: add(scale(self.x, c), scale(self.y, s)),
            y: add(scale(self.x, -s), scale(self.y, c)),
            z: self.z,
        }
    }

    /// A frame whose normal is `dir` (unit), origin at `origin`, with `x`
    /// the projection of `hint` (or any perpendicular when parallel).
    pub fn from_normal(origin: [f64; 3], dir: [f64; 3], hint: [f64; 3]) -> Option<Frame> {
        let l = norm(dir);
        if !(l > 0.0 && l.is_finite()) {
            return None;
        }
        let z = scale(dir, 1.0 / l);
        let mut x = sub(hint, scale(z, dot(hint, z)));
        if norm(x) < 1e-9 {
            let alt = if z[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            x = sub(alt, scale(z, dot(alt, z)));
        }
        let x = scale(x, 1.0 / norm(x));
        Some(Frame {
            origin,
            x,
            y: cross(z, x),
            z,
        })
    }

    pub fn is_finite(&self) -> bool {
        [self.origin, self.x, self.y, self.z]
            .iter()
            .flatten()
            .all(|v| v.is_finite())
    }
}

/// One segment of a [`Profile::Loop`]: from `start` to the next segment's
/// start, straight or along the circle about `center` (radius from `start`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "seg", rename_all = "snake_case")]
pub enum Segment {
    Line {
        start: [f64; 2],
    },
    Arc {
        start: [f64; 2],
        center: [f64; 2],
        ccw: bool,
    },
}

impl Segment {
    pub fn start(&self) -> [f64; 2] {
        match *self {
            Segment::Line { start } | Segment::Arc { start, .. } => start,
        }
    }
}

/// Closed plane profile of an operation (either orientation; normalised
/// before use).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Profile {
    Circle { center: [f64; 2], radius: f64 },
    Polygon { points: Vec<[f64; 2]> },
    Loop { segments: Vec<Segment> },
}

/// Axis-aligned rectangle centred at `c`, `w` × `h`, rotated `a` radians.
pub fn rect(c: [f64; 2], w: f64, h: f64, a: f64) -> Profile {
    let (s, co) = a.sin_cos();
    let pts = [[-w, -h], [w, -h], [w, h], [-w, h]]
        .iter()
        .map(|p| {
            let (x, y) = (p[0] / 2.0, p[1] / 2.0);
            [c[0] + x * co - y * s, c[1] + x * s + y * co]
        })
        .collect();
    Profile::Polygon { points: pts }
}

fn finite2(p: [f64; 2]) -> bool {
    p[0].is_finite() && p[1].is_finite()
}

fn signed_area(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
        / 2.0
}

/// Index of the lowest, then leftmost, point.
fn lowest(pts: impl Iterator<Item = [f64; 2]>) -> usize {
    let mut best = 0;
    let mut bp = [f64::INFINITY, f64::INFINITY];
    for (i, p) in pts.enumerate() {
        if p[1] < bp[1] || (p[1] == bp[1] && p[0] < bp[0]) {
            best = i;
            bp = p;
        }
    }
    best
}

impl Profile {
    /// A sketch-layer profile as a plugin profile (None for free-form spans).
    pub fn from_sketch(p: &SketchProfile) -> Option<Profile> {
        Some(match p {
            SketchProfile::Circle { center, radius } => Profile::Circle {
                center: *center,
                radius: *radius,
            },
            SketchProfile::Polygon(v) => Profile::Polygon { points: v.clone() },
            SketchProfile::Loop(segs) => Profile::Loop {
                segments: segs
                    .iter()
                    .map(|s| match *s {
                        LoopSegment::Line { start } => Some(Segment::Line { start }),
                        LoopSegment::Arc { start, center, ccw } => {
                            Some(Segment::Arc { start, center, ccw })
                        }
                        LoopSegment::Cubic { .. } => None,
                    })
                    .collect::<Option<Vec<_>>>()?,
            },
        })
    }

    /// The profile offset by `d` mm (outward > 0, inward < 0) with the
    /// sketch layer's exact offset (parallel lines, concentric arcs, mitred
    /// sharp corners), or why it collapses.
    pub fn offset(&self, d: f64) -> Result<Profile, String> {
        if d == 0.0 {
            return Ok(self.clone());
        }
        let sp = self.to_sketch()?;
        let o = rcad_sketch::offset_profile(&sp, d).map_err(|e| e.to_string())?;
        Profile::from_sketch(&o).ok_or_else(|| "offset produced a free-form curve".into())
    }

    /// Signed area (counter-clockwise positive) of the canonical profile.
    pub fn area(&self) -> Result<f64, String> {
        Ok(match self.to_sketch()? {
            SketchProfile::Circle { radius, .. } => std::f64::consts::PI * radius * radius,
            SketchProfile::Polygon(v) => signed_area(&v),
            SketchProfile::Loop(s) => rcad_sketch::loop_area(&s),
        })
    }

    /// The profile in the sketch layer's canonical form (counter-clockwise,
    /// starting at the lowest then leftmost vertex), or why it is not a
    /// usable profile (non-finite, degenerate, zero radius, < 3 points).
    pub fn to_sketch(&self) -> Result<SketchProfile, String> {
        match self {
            Profile::Circle { center, radius } => {
                if !(finite2(*center) && radius.is_finite() && *radius > 0.0) {
                    return Err(format!(
                        "circle needs a finite centre and radius > 0, got {radius}"
                    ));
                }
                Ok(SketchProfile::Circle {
                    center: *center,
                    radius: *radius,
                })
            }
            Profile::Polygon { points } => {
                if points.len() < 3 || !points.iter().all(|p| finite2(*p)) {
                    return Err("polygon needs >= 3 finite points".into());
                }
                let mut pts = points.clone();
                let a = signed_area(&pts);
                if a.abs() < 1e-12 {
                    return Err("polygon has no area".into());
                }
                if a < 0.0 {
                    pts.reverse();
                }
                let k = lowest(pts.iter().copied());
                pts.rotate_left(k);
                Ok(SketchProfile::Polygon(pts))
            }
            Profile::Loop { segments } => {
                if segments.len() < 2 || !segments.iter().all(|s| finite2(s.start())) {
                    return Err("loop needs >= 2 finite segments".into());
                }
                if !segments.iter().any(|s| matches!(s, Segment::Arc { .. })) {
                    return Profile::Polygon {
                        points: segments.iter().map(|s| s.start()).collect(),
                    }
                    .to_sketch();
                }
                let mut segs: Vec<LoopSegment> = segments
                    .iter()
                    .map(|s| match *s {
                        Segment::Line { start } => LoopSegment::Line { start },
                        Segment::Arc { start, center, ccw } => {
                            LoopSegment::Arc { start, center, ccw }
                        }
                    })
                    .collect();
                let a = rcad_sketch::loop_area(&segs);
                if a.abs() < 1e-12 {
                    return Err("loop has no area".into());
                }
                if a < 0.0 {
                    // Reverse: segment i runs from s_i to s_{i+1}; reversed it
                    // runs from s_{i+1} to s_i with the opposite turn.
                    let n = segs.len();
                    let starts: Vec<[f64; 2]> = segs
                        .iter()
                        .map(|s| match s {
                            LoopSegment::Line { start } | LoopSegment::Arc { start, .. } => *start,
                            LoopSegment::Cubic { start, .. } => *start,
                        })
                        .collect();
                    let mut rev = Vec::with_capacity(n);
                    for i in (0..n).rev() {
                        let end = starts[(i + 1) % n];
                        rev.push(match segs[i] {
                            LoopSegment::Arc { center, ccw, .. } => LoopSegment::Arc {
                                start: end,
                                center,
                                ccw: !ccw,
                            },
                            _ => LoopSegment::Line { start: end },
                        });
                    }
                    segs = rev;
                }
                let k = lowest(segs.iter().map(|s| s.start()));
                segs.rotate_left(k);
                Ok(SketchProfile::Loop(segs))
            }
        }
    }
}

/// A resolved planar face: its frame (origin on the face, `z` the outward
/// normal), the body it belongs to, its boundary in plane coordinates
/// (`loops[0]` the outer loop, then the inner loops; exact lines and arcs,
/// empty when the boundary has other curves), and the extent of the whole
/// body along `z` (`[lo, hi]`, heights relative to the face: `lo < 0` is the
/// material depth below a face on top).
///
/// `body_span` comes from the body's bounding box (conservative for
/// directions that are not axis-aligned); `parallel` lists the body's planar
/// faces parallel to this one, exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceGeom {
    pub frame: Frame,
    pub body: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<Profile>,
    pub body_span: [f64; 2],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parallel: Vec<ParallelPlane>,
}

/// A planar face of a body parallel to a reference plane: its height along
/// the reference `z` and the sign of its outward normal along `z` (`+1`: it
/// faces up the reference normal, e.g. a floor seen from above; `-1`: it
/// faces down, e.g. a ceiling). Faces in one plane with the same facing
/// appear once.
///
/// `footprint` is the bounding box `[min, max]` of the face's boundary in
/// the reference plane's coordinates (each face is listed on its own), so
/// "to next" can be limited to faces under a tool.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ParallelPlane {
    pub height: f64,
    pub facing: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<[[f64; 2]; 2]>,
}

/// Plane-coordinate box `[min, max]`.
pub type Footprint = [[f64; 2]; 2];

fn overlaps(p: &ParallelPlane, over: Option<&Footprint>) -> bool {
    match (p.footprint, over) {
        (Some(f), Some(o)) => {
            f[0][0] <= o[1][0] && o[0][0] <= f[1][0] && f[0][1] <= o[1][1] && o[0][1] <= f[1][1]
        }
        _ => true,
    }
}

/// [`next_below`] among the faces whose footprint overlaps `over`.
pub fn next_below_over(
    planes: &[ParallelPlane],
    h: f64,
    eps: f64,
    over: &Footprint,
) -> Option<f64> {
    planes
        .iter()
        .filter(|p| p.facing > 0.0 && p.height < h - eps && overlaps(p, Some(over)))
        .map(|p| p.height)
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
}

/// [`next_above`] among the faces whose footprint overlaps `over`.
pub fn next_above_over(
    planes: &[ParallelPlane],
    h: f64,
    eps: f64,
    over: &Footprint,
) -> Option<f64> {
    planes
        .iter()
        .filter(|p| p.facing < 0.0 && p.height > h + eps && overlaps(p, Some(over)))
        .map(|p| p.height)
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
}

/// The nearest face below `h` facing away (down the reference normal) whose
/// footprint overlaps `over`: e.g. the outside of a floor under a tool.
pub fn back_below_over(
    planes: &[ParallelPlane],
    h: f64,
    eps: f64,
    over: &Footprint,
) -> Option<f64> {
    planes
        .iter()
        .filter(|p| p.facing < 0.0 && p.height < h - eps && overlaps(p, Some(over)))
        .map(|p| p.height)
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
}

/// Bounding box of points, grown by `r`.
pub fn footprint_of(points: &[[f64; 2]], r: f64) -> Footprint {
    let mut f = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in points {
        for i in 0..2 {
            f[0][i] = f[0][i].min(p[i] - r);
            f[1][i] = f[1][i].max(p[i] + r);
        }
    }
    f
}

/// The nearest parallel plane below height `h` (strictly, by more than
/// `eps`) facing up: the "next" face going down the reference normal.
pub fn next_below(planes: &[ParallelPlane], h: f64, eps: f64) -> Option<f64> {
    planes
        .iter()
        .filter(|p| p.facing > 0.0 && p.height < h - eps)
        .map(|p| p.height)
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
}

/// The nearest parallel plane above height `h` facing down.
pub fn next_above(planes: &[ParallelPlane], h: f64, eps: f64) -> Option<f64> {
    planes
        .iter()
        .filter(|p| p.facing < 0.0 && p.height > h + eps)
        .map(|p| p.height)
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
}

/// A resolved body: id and world bounding box (`[min, max]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyGeom {
    pub body: u32,
    pub bounds: [[f64; 3]; 2],
}

/// A curve of a resolved sketch, in the sketch's plane coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "curve", rename_all = "snake_case")]
pub enum Curve {
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        end: f64,
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
}

/// A resolved sketch: frame, its solved non-construction curves, its
/// standalone points (used by no curve; not the sketch origin), and the body
/// whose face it lies on, if any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchGeom {
    pub frame: Frame,
    pub curves: Vec<Curve>,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<u32>,
    /// Planar faces of `body` parallel to the sketch plane (see
    /// [`ParallelPlane`]); empty without a body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parallel: Vec<ParallelPlane>,
}

/// Geometry of one input as resolved at this point of the history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resolved {
    Face(FaceGeom),
    Faces { faces: Vec<FaceGeom> },
    Body(BodyGeom),
    Sketch(SketchGeom),
    Points { points: Vec<[f64; 2]> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clockwise_polygons_become_counter_clockwise_from_the_lowest_vertex() {
        let p = Profile::Polygon {
            points: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        };
        let SketchProfile::Polygon(q) = p.to_sketch().unwrap() else {
            panic!()
        };
        assert_eq!(q, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    }

    #[test]
    fn reversed_loops_flip_their_arcs() {
        let half = Profile::Loop {
            segments: vec![
                Segment::Line { start: [1.0, 0.0] },
                Segment::Arc {
                    start: [-1.0, 0.0],
                    center: [0.0, 0.0],
                    ccw: false,
                },
            ],
        };
        // [1,0] -> [-1,0] straight, then cw arc from [-1,0] over the top back to
        // [1,0]: clockwise overall (area -pi/2).
        let SketchProfile::Loop(s) = half.to_sketch().unwrap() else {
            panic!()
        };
        assert!(rcad_sketch::loop_area(&s) > 0.0);
        assert!((rcad_sketch::loop_area(&s) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn frames_round_trip_points() {
        let f = Frame::from_normal([1.0, 2.0, 3.0], [0.0, 0.0, 2.0], [1.0, 0.0, 0.0]).unwrap();
        let q = f.point([3.0, 4.0], 5.0);
        assert_eq!(q, [4.0, 6.0, 8.0]);
        let (p, h) = f.local(q);
        assert_eq!((p, h), ([3.0, 4.0], 5.0));
        assert_eq!(f.flipped().z, [0.0, 0.0, -1.0]);
    }
}
