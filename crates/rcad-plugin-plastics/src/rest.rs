//! Rest (docs/PLASTICS.md "Rest"): a flat platform inside a shelled part
//! with walls of uniform thickness, so it makes no thick section.
//!
//! For each closed profile of the sketch (circles, line loops) on a plane
//! above a floor of the body: the profile, drafted by `draft_deg` (exact at
//! the top, growing towards the floor), is joined from the floor (the
//! nearest face below the sketch plane facing it, `to_next`; or `depth`
//! below the sketch plane) up to the top at `offset` above the sketch plane;
//! then its inside, `thickness` in from the walls, is cut from below the
//! floor up to `thickness` under the top. The platform is a cap with walls
//! whose inside opens through the floor (uniform wall, no closed void).
//! Operation ids `4·k + part` (0 solid, 1 inside) per profile `k`.
//!
//! Wall thickness is measured across the walls in the sketch plane.
//! Not modelled: Fusion's top-face limits, trimming at the body and
//! per-side drafts.
use crate::util::{EPS, closed_profiles, invalid, prism};
use crate::{choice, input, param};
use rcad_plugin::geom::{Footprint, back_below_over, footprint_of, next_below_over};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, InputKind, Mode, Op, ParamKind, PluginError};

const F: &str = "rest";

pub fn schema() -> FeatureSchema {
    let pos = Some((0.0, true));
    FeatureSchema {
        id: F.into(),
        name: "Rest".into(),
        description: "Flat platform with uniform walls inside a shelled part".into(),
        inputs: vec![input("sketch", "Sketch profiles", InputKind::Sketch, false)],
        params: vec![
            param(
                "thickness",
                "Wall thickness",
                ParamKind::Length,
                2.0,
                pos,
                None,
                Some("T"),
            ),
            param(
                "offset",
                "Top offset",
                ParamKind::Length,
                0.0,
                None,
                None,
                None,
            ),
            param(
                "depth",
                "Depth below the sketch (distance)",
                ParamKind::Length,
                10.0,
                pos,
                None,
                None,
            ),
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
        choices: vec![choice("extent", "Extent", &["to_next", "distance"])],
    }
}

fn profile_footprint(p: &rcad_plugin::Profile) -> Footprint {
    use rcad_plugin::Profile as P;
    match p {
        P::Circle { center, radius } => footprint_of(&[*center], *radius),
        P::Polygon { points } => footprint_of(points, 0.0),
        P::Loop { segments } => {
            let pts: Vec<[f64; 2]> = segments.iter().map(|s| s.start()).collect();
            footprint_of(&pts, 0.0)
        }
    }
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let s = req.sketch("sketch")?;
    if s.body.is_none() {
        return Err(invalid(
            F,
            "the sketch must lie on a plane offset from a face of the body",
        ));
    }
    let (t, o, a) = (
        req.param("thickness")?,
        req.param("offset")?,
        req.param("draft_deg")?.to_radians(),
    );
    let mut ops = Vec::new();
    for (k, p) in closed_profiles(F, s)?.into_iter().enumerate() {
        let id = 4 * k as u32;
        // Floor height below the sketch plane (< 0) under this profile, and
        // the outside face under it.
        let fp = profile_footprint(&p);
        let floor = match req.choice("extent")? {
            "to_next" => next_below_over(&s.parallel, 0.0, EPS, &fp).ok_or_else(|| {
                invalid(
                    F,
                    format!(
                        "to next: no face of the body under rest {} faces the sketch plane",
                        k + 1
                    ),
                )
            })?,
            _ => -req.param("depth")?,
        };
        let bottom = back_below_over(&s.parallel, floor, EPS, &fp)
            .ok_or_else(|| invalid(F, "no outside face of the body under the floor"))?;
        let top = o;
        let rise = top - floor;
        if rise <= t + EPS {
            return Err(invalid(
                F,
                format!(
                    "the top is {rise:.3} mm above the floor; it must be more than the thickness"
                ),
            ));
        }
        let base = s.frame.offset(floor);
        let lbl = |w: &str| format!("rest {}: {w}", k + 1);
        // Solid: exact at the top, `rise·tan(a)` wider at the floor.
        let solid = p.offset(rise * a.tan()).map_err(|e| invalid(F, e))?;
        ops.push(prism(
            id,
            "sketch",
            base,
            solid.clone(),
            vec![],
            (0.0, rise),
            a.to_degrees(),
            Mode::Join,
            lbl("walls"),
        ));
        // Inside: from below the floor (height `bottom - 1`) to `t` under the top.
        let from = bottom - 1.0 - floor;
        let inside = p
            .offset((top - (floor + from)) * a.tan() - t)
            .map_err(|e| {
                invalid(
                    F,
                    format!("the thickness {t} mm does not fit the profile ({e})"),
                )
            })?;
        ops.push(prism(
            id + 1,
            "sketch",
            base,
            inside,
            vec![],
            (from, rise - t),
            a.to_degrees(),
            Mode::Cut,
            lbl("inside"),
        ));
    }
    Ok(ops)
}
