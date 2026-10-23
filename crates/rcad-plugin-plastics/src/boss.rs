//! Boss (docs/PLASTICS.md "Boss"): screw bosses at sketch points, with an
//! optional pilot or through hole, gusset ribs and a mating boss with a
//! clearance hole and counterbore on a second body (Fusion's Side 1 /
//! Side 2).
//!
//! Per placement point `i` (operation ids `32·i + part`, stable while points
//! keep their order):
//! Operations run shank, ribs, hole (the hole is cut last so ribs, which
//! reach the axis, do not fill it again).
//!
//! * part 0: shank, a circle of `diameter` joined from the base face (the
//!   nearest face of the sketch's body at or below the sketch plane, facing
//!   it) up `height`, drafted by `draft_deg` (diameter at the root);
//! * part 1: hole, `hole_diameter` at the top, `hole_depth` deep from the top
//!   (`hole` = pilot) or through the body (`through`), drafted by
//!   `hole_draft_deg` (widening towards the top);
//! * parts 2..: gusset ribs, a section from the axis to `rib_length` with
//!   `rib_height` at the shank sloping to `rib_tip_height` at the end,
//!   `rib_thickness` thick, sunk [`SINK`] into the base, evenly over
//!   `rib_total_deg` from `rib_angle_deg` (sketch x axis);
//! * parts 16..18 (with a `mate` face): a mating shank of `mate_diameter`
//!   on the mate face up to `gap` short of the shank's top, a clearance hole
//!   of `mate_hole_diameter` through the mate body and a counterbore of
//!   `cbore_diameter` × `cbore_depth` from its outside face.
//!
//! Not supported (refused or absent, stated): ribs on a drafted shank
//! (cone/plane joins), root/tip fillets, drafted ribs, countersinks, alignment steps, a fastener component.
use crate::util::{EPS, SINK, invalid, placement_points, prism, standing, unsupported};
use crate::{choice, input, param};
use rcad_plugin::geom::{back_below_over, dot, footprint_of, next_below_over};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, Frame, InputKind, Mode, Op, ParamKind, PluginError, Profile};

const F: &str = "boss";

pub fn schema() -> FeatureSchema {
    use ParamKind::{Angle, Count, Length};
    let pos = Some((0.0, true));
    let nonneg = Some((0.0, false));
    FeatureSchema {
        id: F.into(),
        name: "Boss".into(),
        description: "Screw bosses at sketch points, with hole, ribs and an optional mating boss"
            .into(),
        inputs: vec![
            input("sketch", "Sketch points", InputKind::Sketch, false),
            input("mate", "Mating face (side 2)", InputKind::Face, true),
        ],
        params: vec![
            param("height", "Height", Length, 8.0, pos, None, None),
            param("diameter", "Outer diameter", Length, 6.0, pos, None, None),
            param(
                "draft_deg",
                "Draft",
                Angle,
                1.0,
                nonneg,
                Some(30.0),
                Some("DA"),
            ),
            param(
                "hole_diameter",
                "Hole diameter",
                Length,
                2.5,
                pos,
                None,
                None,
            ),
            param("hole_depth", "Hole depth", Length, 6.0, pos, None, None),
            param(
                "hole_draft_deg",
                "Hole draft",
                Angle,
                0.0,
                nonneg,
                Some(30.0),
                None,
            ),
            param("rib_count", "Ribs", Count, 0.0, nonneg, Some(12.0), None),
            param(
                "rib_thickness",
                "Rib thickness",
                Length,
                1.2,
                pos,
                None,
                Some("0.6*T"),
            ),
            param(
                "rib_length",
                "Rib length (from axis)",
                Length,
                6.0,
                pos,
                None,
                None,
            ),
            param("rib_height", "Rib height", Length, 6.0, pos, None, None),
            param(
                "rib_tip_height",
                "Rib height at its end",
                Length,
                2.0,
                pos,
                None,
                None,
            ),
            param(
                "rib_angle_deg",
                "Rib rotation",
                Angle,
                0.0,
                None,
                None,
                None,
            ),
            param(
                "rib_total_deg",
                "Rib total angle",
                Angle,
                360.0,
                pos,
                Some(360.0),
                None,
            ),
            param(
                "mate_diameter",
                "Mating outer diameter",
                Length,
                6.0,
                pos,
                None,
                None,
            ),
            param(
                "mate_hole_diameter",
                "Clearance hole",
                Length,
                3.4,
                pos,
                None,
                None,
            ),
            param(
                "cbore_diameter",
                "Counterbore diameter",
                Length,
                6.0,
                pos,
                None,
                None,
            ),
            param(
                "cbore_depth",
                "Counterbore depth (0: none)",
                Length,
                0.0,
                nonneg,
                None,
                None,
            ),
            param(
                "gap",
                "Gap between the bosses",
                Length,
                0.0,
                nonneg,
                None,
                Some("CL"),
            ),
        ],
        choices: vec![choice("hole", "Hole", &["pilot", "through", "none"])],
    }
}

fn circle(r: f64) -> Profile {
    Profile::Circle {
        center: [0.0, 0.0],
        radius: r,
    }
}

/// Diameter at height `from` of a hole that is `d` wide at height `top`
/// and widens upwards by `a` (radians).
fn hole_at(d: f64, top: f64, from: f64, a: f64) -> f64 {
    d - 2.0 * (top - from) * a.tan()
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let p = |n: &str| req.param(n);
    let s = req.sketch("sketch")?;
    let body = s.body.ok_or_else(|| {
        invalid(
            F,
            "the sketch must lie on a face of the boss body (or a plane offset from one)",
        )
    })?;
    let _ = body;
    let pts = placement_points(s);
    if pts.is_empty() {
        return Err(invalid(
            F,
            "the sketch has no points (or circles) to place bosses at",
        ));
    }
    let (h, d, da) = (p("height")?, p("diameter")?, p("draft_deg")?.to_radians());
    let (hd, hdepth, hda) = (
        p("hole_diameter")?,
        p("hole_depth")?,
        p("hole_draft_deg")?.to_radians(),
    );
    let hole = req.choice("hole")?;
    let top_d = d - 2.0 * h * da.tan();
    if top_d <= hd + EPS && hole != "none" {
        return Err(invalid(
            F,
            format!("the draft leaves {top_d:.3} mm at the top, not wider than the {hd} mm hole"),
        ));
    }
    if top_d <= EPS {
        return Err(invalid(F, "the draft closes the shank before its top"));
    }
    let ribs = p("rib_count")? as usize;
    let (rt, rl, rh, rtip) = (
        p("rib_thickness")?,
        p("rib_length")?,
        p("rib_height")?,
        p("rib_tip_height")?,
    );
    if ribs > 0 {
        if da > 0.0 {
            return Err(unsupported(
                F,
                "gusset ribs on a drafted shank are not supported (the kernel cannot join planar \
                 ribs to a cone); set draft_deg to 0 or rib_count to 0",
            ));
        }
        if rl <= d / 2.0 + EPS {
            return Err(invalid(
                F,
                "rib_length must reach beyond the shank (> diameter / 2)",
            ));
        }
        if rh > h + EPS || rtip > rh + EPS {
            return Err(invalid(F, "ribs need rib_tip_height ≤ rib_height ≤ height"));
        }
        if rt >= d {
            return Err(invalid(F, "rib_thickness must be less than the diameter"));
        }
    }
    let mate = if req.has("mate") {
        Some(req.face("mate")?)
    } else {
        None
    };
    let mut ops = Vec::new();
    for (i, pt) in pts.iter().enumerate() {
        let id = 32 * i as u32;
        // Base: the nearest face at or below the sketch plane facing it under
        // this boss; bottom: the outside face under that.
        let reach = if ribs > 0 { rl.max(d / 2.0) } else { d / 2.0 };
        let fp = footprint_of(&[*pt], d / 2.0);
        let base = next_below_over(&s.parallel, EPS, 0.0, &fp).ok_or_else(|| {
            invalid(
                F,
                format!(
                    "boss {}: no face of the body at or below the sketch plane faces it",
                    i + 1
                ),
            )
        })?;
        let bottom = back_below_over(&s.parallel, base, EPS, &footprint_of(&[*pt], reach));
        if ribs > 0 && !bottom.is_some_and(|b| base - b > 2.0 * SINK) {
            return Err(unsupported(
                F,
                format!("ribs sink {SINK} mm into the base; the material under it is too thin"),
            ));
        }
        let top = base + h;
        let at = s.frame.at(*pt).offset(base);
        let lbl = |w: &str| format!("boss {}: {w}", i + 1);
        ops.push(prism(
            id,
            "sketch",
            at,
            circle(d / 2.0),
            vec![],
            (0.0, h),
            da.to_degrees(),
            Mode::Join,
            lbl("shank"),
        ));
        if ribs > 0 {
            let total = p("rib_total_deg")?.to_radians();
            let step = if (total - std::f64::consts::TAU).abs() < 1e-9 {
                total / ribs as f64
            } else if ribs > 1 {
                total / (ribs - 1) as f64
            } else {
                0.0
            };
            let a0 = p("rib_angle_deg")?.to_radians();
            for k in 0..ribs {
                let f = standing(&s.frame.offset(base), *pt, a0 + step * k as f64);
                let section = Profile::Polygon {
                    points: vec![[0.0, -SINK], [rl, -SINK], [rl, rtip], [0.0, rh]],
                };
                ops.push(prism(
                    id + 2 + k as u32,
                    "sketch",
                    f,
                    section,
                    vec![],
                    (-rt / 2.0, rt / 2.0),
                    0.0,
                    Mode::Join,
                    lbl(&format!("rib {}", k + 1)),
                ));
            }
        }
        match hole {
            "pilot" => {
                if hdepth > h + base - bottom.unwrap_or(f64::NEG_INFINITY) - EPS {
                    return Err(invalid(
                        F,
                        "the pilot hole is deeper than the material; use a through hole",
                    ));
                }
                let from = h - hdepth;
                let dd = hole_at(hd, h, from, hda);
                if dd <= EPS {
                    return Err(invalid(
                        F,
                        "the hole draft closes the hole before its bottom",
                    ));
                }
                ops.push(prism(
                    id + 1,
                    "sketch",
                    at,
                    circle(dd / 2.0),
                    vec![],
                    (from, h + 1.0),
                    -hda.to_degrees(),
                    Mode::Cut,
                    lbl("hole"),
                ));
            }
            "through" => {
                let b = bottom.ok_or_else(|| {
                    unsupported(
                        F,
                        "a through hole needs a face of the body under the base facing away",
                    )
                })? - base;
                let from = b - 1.0;
                let dd = hole_at(hd, h, from, hda);
                if dd <= EPS {
                    return Err(invalid(
                        F,
                        "the hole draft closes the hole before its bottom",
                    ));
                }
                ops.push(prism(
                    id + 1,
                    "sketch",
                    at,
                    circle(dd / 2.0),
                    vec![],
                    (from, h + 1.0),
                    -hda.to_degrees(),
                    Mode::Cut,
                    lbl("hole"),
                ));
            }
            _ => {}
        }
        if let Some(m) = mate {
            mate_ops(req, &mut ops, id, s.frame.point(*pt, top), &s.frame, m, i)?;
        }
    }
    Ok(ops)
}

fn mate_ops(
    req: &BuildRequest,
    ops: &mut Vec<Op>,
    id: u32,
    top: [f64; 3],
    sketch: &Frame,
    m: &rcad_plugin::FaceGeom,
    i: usize,
) -> Result<(), PluginError> {
    let p = |n: &str| req.param(n);
    if (dot(sketch.z, m.frame.z) + 1.0).abs() > 1e-9 {
        return Err(invalid(
            F,
            "the mating face must be parallel to the sketch plane and face it",
        ));
    }
    let (uv, top_h) = m.frame.local(top);
    let len = top_h - p("gap")?;
    if len <= EPS {
        return Err(invalid(
            F,
            format!(
                "the mating face is {top_h:.3} mm from the boss tops; nothing is left for the mating boss"
            ),
        ));
    }
    let (md, mh, cd, cdepth) = (
        p("mate_diameter")?,
        p("mate_hole_diameter")?,
        p("cbore_diameter")?,
        p("cbore_depth")?,
    );
    let da = p("draft_deg")?;
    if mh >= md - 2.0 * len * da.to_radians().tan() - EPS {
        return Err(invalid(
            F,
            "the clearance hole is not narrower than the mating boss",
        ));
    }
    // The outside face of the mate body (facing away from the bosses).
    let out = back_below_over(&m.parallel, 0.0, EPS, &footprint_of(&[uv], mh / 2.0)).ok_or_else(
        || {
            unsupported(
                F,
                "the mate body has no outside face parallel to the mating face",
            )
        },
    )?;
    let at = m.frame.at(uv);
    let lbl = |w: &str| format!("boss {} (mate): {w}", i + 1);
    let c = |r: f64| Profile::Circle {
        center: [0.0, 0.0],
        radius: r,
    };
    ops.push(prism(
        id + 16,
        "mate",
        at,
        c(md / 2.0),
        vec![],
        (0.0, len),
        da,
        Mode::Join,
        lbl("shank"),
    ));
    ops.push(prism(
        id + 17,
        "mate",
        at,
        c(mh / 2.0),
        vec![],
        (out - 1.0, len + 1.0),
        0.0,
        Mode::Cut,
        lbl("clearance hole"),
    ));
    if cdepth > 0.0 {
        if cd <= mh + EPS {
            return Err(invalid(
                F,
                "the counterbore must be wider than the clearance hole",
            ));
        }
        if cdepth >= -out - EPS {
            return Err(invalid(F, "the counterbore is deeper than the mate wall"));
        }
        ops.push(prism(
            id + 18,
            "mate",
            at,
            c(cd / 2.0),
            vec![],
            (out - 1.0, out + cdepth),
            0.0,
            Mode::Cut,
            lbl("counterbore"),
        ));
    }
    Ok(())
}
