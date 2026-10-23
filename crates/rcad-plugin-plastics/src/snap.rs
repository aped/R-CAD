//! Snap Fit (docs/PLASTICS.md "Snap fit"): cantilever hooks at sketch
//! points, with a mating groove (window) or loop on a second body.
//!
//! Section of a hook in the plane standing on the sketch plane at the point,
//! along the sketch direction at `rotation_deg` (or, with `orient`
//! outward/inward, away from/towards the points' centroid; `x`: the side the catch
//! points to; `y`: the sketch normal), `width` wide across it:
//! * beam from `x = 0` to its thickness, `thickness` at the root tapering to
//!   `tip_thickness` at `length` above the sketch plane;
//! * catch on the beam's `+x` side: `catch_height` long ending at the tip,
//!   protruding `catch_depth`, with a retention face at `retention_deg` and
//!   an insertion face at `insertion_deg` to the beam axis (90° retention: a
//!   flat ledge, not releasable);
//! * root: `perpendicular` hooks stand on the sketch's face (sunk 0.2 mm
//!   into it); `parallel` hooks stand off a lower face, the beam continuing
//!   from the sketch plane down to the nearest face facing it (`to_next`) or
//!   `base_depth`.
//!
//! With a `mate` body: `hook_and_groove` cuts the catch's envelope grown by
//! `clearance` from it (the window the catch snaps into); `hook_and_loop`
//! joins a bar `loop_thickness` thick under the retention face, from
//! `clearance` beside the beam out `loop_length` past the catch, which must
//! reach the mate body. Operation ids `8·i + part` (0 hook, 1 groove/loop).
//!
//! Not modelled: base fillet, deflection/strain check (Fusion has none
//! either).
use crate::util::{EPS, SINK, invalid, placement_points, prism, standing};
use crate::{choice, input, param};
use rcad_plugin::geom::{footprint_of, next_below_over};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, InputKind, Mode, Op, ParamKind, PluginError, Profile};

const F: &str = "snap_fit";

pub fn schema() -> FeatureSchema {
    use ParamKind::{Angle, Length};
    let pos = Some((0.0, true));
    FeatureSchema {
        id: F.into(),
        name: "Snap Fit".into(),
        description: "Cantilever hooks at sketch points with a mating groove or loop".into(),
        inputs: vec![
            input("sketch", "Sketch points", InputKind::Sketch, false),
            input(
                "mate",
                "Mating body (groove or loop)",
                InputKind::Body,
                true,
            ),
        ],
        params: vec![
            param("length", "Beam length", Length, 8.0, pos, None, None),
            param(
                "thickness",
                "Beam thickness at the root",
                Length,
                1.0,
                pos,
                None,
                Some("T/2"),
            ),
            param(
                "tip_thickness",
                "Beam thickness at the tip",
                Length,
                1.0,
                pos,
                None,
                Some("T/2"),
            ),
            param("width", "Width", Length, 5.0, pos, None, None),
            param(
                "catch_depth",
                "Catch depth",
                Length,
                0.7,
                pos,
                None,
                Some("T/3"),
            ),
            param(
                "catch_height",
                "Catch height",
                Length,
                2.0,
                pos,
                None,
                Some("T"),
            ),
            param(
                "insertion_deg",
                "Insertion angle",
                Angle,
                30.0,
                Some((0.0, true)),
                Some(80.0),
                None,
            ),
            param(
                "retention_deg",
                "Retention angle",
                Angle,
                90.0,
                Some((0.0, true)),
                Some(90.0),
                None,
            ),
            param("rotation_deg", "Rotation", Angle, 0.0, None, None, None),
            param(
                "base_depth",
                "Stand-off depth (parallel, distance)",
                Length,
                5.0,
                pos,
                None,
                None,
            ),
            param(
                "clearance",
                "Clearance",
                Length,
                0.1,
                Some((0.0, false)),
                None,
                Some("CL"),
            ),
            param(
                "loop_thickness",
                "Loop thickness",
                Length,
                1.5,
                pos,
                None,
                None,
            ),
            param(
                "loop_length",
                "Loop length past the catch",
                Length,
                3.0,
                pos,
                None,
                None,
            ),
        ],
        choices: vec![
            choice("type", "Type", &["hook_and_groove", "hook_and_loop"]),
            choice("orientation", "Hook", &["perpendicular", "parallel"]),
            choice("extent", "Stand-off (parallel)", &["to_next", "distance"]),
            choice("orient", "Rotation", &["uniform", "outward", "inward"]),
        ],
    }
}

/// Hook section points and the catch's extents.
struct Hook {
    section: Vec<[f64; 2]>,
    /// Beam outer x at the retention face, catch outer x.
    x_root: f64,
    x_out: f64,
    /// Retention face bottom, tip height.
    v0: f64,
    tip: f64,
    width: f64,
}

fn hook(req: &BuildRequest, y0: f64) -> Result<Hook, PluginError> {
    let p = |n: &str| req.param(n);
    let (l, a, c, w) = (
        p("length")?,
        p("thickness")?,
        p("tip_thickness")?,
        p("width")?,
    );
    let (fd, h) = (p("catch_depth")?, p("catch_height")?);
    let (ins, ret) = (
        p("insertion_deg")?.to_radians(),
        p("retention_deg")?.to_radians(),
    );
    if h >= l - EPS {
        return Err(invalid(F, "the catch must be shorter than the beam"));
    }
    let xc = |y: f64| a + (c - a) * y / l;
    let v0 = l - h;
    let x0 = xc(v0);
    let xo = x0 + fd;
    let v1 = v0 + fd / ret.tan();
    let run = xo - c;
    let v2 = l - run / ins.tan();
    if v2 < v1 - EPS {
        return Err(invalid(
            F,
            format!(
                "the catch is too short for its angles: retention and insertion faces need {:.3} mm, the catch is {h} mm",
                (v1 - v0) + (l - v2)
            ),
        ));
    }
    let mut pts = vec![[0.0, y0], [a, y0]];
    if (c - a).abs() > EPS {
        pts.push([a, 0.0]);
    }
    pts.push([x0, v0]);
    pts.push([xo, v1]);
    if v2 > v1 + EPS {
        pts.push([xo, v2]);
    }
    pts.push([c, l]);
    pts.push([0.0, l]);
    Ok(Hook {
        section: pts,
        x_root: x0.max(c),
        x_out: xo,
        v0,
        tip: l,
        width: w,
    })
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let p = |n: &str| req.param(n);
    let s = req.sketch("sketch")?;
    if s.body.is_none() {
        return Err(invalid(
            F,
            "the sketch must lie on a face of the hook body (or a plane offset from one)",
        ));
    }
    let pts = placement_points(s);
    if pts.is_empty() {
        return Err(invalid(F, "the sketch has no points to place snap fits at"));
    }
    // Faces under the hooks (their width around each point).
    let fp = footprint_of(&pts, p("width")?.max(p("thickness")?));
    let y0 = match req.choice("orientation")? {
        "parallel" => {
            let d = match req.choice("extent")? {
                "to_next" => -next_below_over(&s.parallel, 0.0, EPS, &fp).ok_or_else(|| {
                    invalid(
                        F,
                        "to next: no face of the body below the sketch plane faces it",
                    )
                })?,
                _ => p("base_depth")?,
            };
            -(d + SINK)
        }
        _ => {
            if next_below_over(&s.parallel, EPS, 0.0, &fp).is_none_or(|h| h.abs() > EPS) {
                return Err(invalid(
                    F,
                    "a perpendicular hook stands on the face its sketch lies on",
                ));
            }
            -SINK
        }
    };
    let hk = hook(req, y0)?;
    let cl = p("clearance")?;
    let rot = p("rotation_deg")?.to_radians();
    // Uniform: every catch along `rotation_deg`; outward / inward: each
    // catch away from / towards the centroid of the points (plus the
    // rotation), e.g. hooks round a box catching its walls.
    let orient = req.choice("orient")?;
    let n = pts.len() as f64;
    let centroid = [
        pts.iter().map(|p| p[0]).sum::<f64>() / n,
        pts.iter().map(|p| p[1]).sum::<f64>() / n,
    ];
    let mate = req.has("mate");
    let kind = req.choice("type")?;
    let mut ops = Vec::new();
    for (i, pt) in pts.iter().enumerate() {
        let id = 8 * i as u32;
        let a = match orient {
            "uniform" => rot,
            o => {
                let (dx, dy) = (pt[0] - centroid[0], pt[1] - centroid[1]);
                if dx.hypot(dy) < EPS {
                    return Err(invalid(
                        F,
                        format!(
                            "snap fit {}: at the centroid of the points, it has no outward direction",
                            i + 1
                        ),
                    ));
                }
                dy.atan2(dx)
                    + rot
                    + if o == "inward" {
                        std::f64::consts::PI
                    } else {
                        0.0
                    }
            }
        };
        let f = standing(&s.frame, *pt, a);
        let half = hk.width / 2.0;
        let lbl = |w: &str| format!("snap fit {}: {w}", i + 1);
        ops.push(prism(
            id,
            "sketch",
            f,
            Profile::Polygon {
                points: hk.section.clone(),
            },
            vec![],
            (-half, half),
            0.0,
            Mode::Join,
            lbl("hook"),
        ));
        if !mate {
            continue;
        }
        if kind == "hook_and_groove" {
            let x_in = hk.x_root.min(hk.x_out) - cl;
            let win = rcad_plugin::geom::rect(
                [
                    (x_in + hk.x_out + cl) / 2.0,
                    (hk.v0 - cl + hk.tip + cl) / 2.0,
                ],
                hk.x_out + cl - x_in,
                hk.tip - hk.v0 + 2.0 * cl,
                0.0,
            );
            ops.push(prism(
                id + 1,
                "mate",
                f,
                win,
                vec![],
                (-half - cl, half + cl),
                0.0,
                Mode::Cut,
                lbl("groove"),
            ));
        } else {
            let (lt, ll) = (p("loop_thickness")?, p("loop_length")?);
            let x0 = hk.x_root + cl;
            let x1 = hk.x_out + ll;
            let y1 = hk.v0 - cl;
            if y1 - lt <= 0.0 {
                return Err(invalid(
                    F,
                    "the loop does not fit between the sketch plane and the catch",
                ));
            }
            let bar = rcad_plugin::geom::rect([(x0 + x1) / 2.0, y1 - lt / 2.0], x1 - x0, lt, 0.0);
            ops.push(prism(
                id + 1,
                "mate",
                f,
                bar,
                vec![],
                (-half - cl - lt, half + cl + lt),
                0.0,
                Mode::Join,
                lbl("loop"),
            ));
        }
    }
    Ok(ops)
}
