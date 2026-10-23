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
use crate::util::{EPS, invalid, prism, unsupported};
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
            input("mate", "Mating rim face (lip and groove)", InputKind::Face, true),
        ],
        params: vec![
            param("width", "Width", ParamKind::Length, 1.0, pos, None, Some("T/2")),
            param("height", "Height", ParamKind::Length, 2.0, pos, None, Some("RH")),
            param("clearance", "Clearance", ParamKind::Length, 0.1, Some((0.0, false)), None, Some("CL")),
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
        [] => Err(unsupported(F, format!("the {what} face's boundary is not made of lines and arcs"))),
        l => Err(invalid(
            F,
            format!("the {what} face must be a rim (one outer and one inner boundary), it has {} loops", l.len()),
        )),
    }
}

fn off(p: &Profile, d: f64, what: &str) -> Result<Profile, PluginError> {
    p.offset(d)
        .map_err(|e| invalid(F, format!("{what}: the offset by {d} mm does not fit ({e})")))
}

/// The band region (outer boundary, hole) `w` wide grown by `g` each side.
fn band(face: &FaceGeom, inner: bool, w: f64, g: f64, what: &str) -> Result<(Profile, Profile), PluginError> {
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
        off(&i, w, what)?.area().map_err(|e| invalid(F, e))? - i.area().map_err(|e| invalid(F, e))?
    } else {
        o.area().map_err(|e| invalid(F, e))? - off(&o, -w, what)?.area().map_err(|e| invalid(F, e))?
    };
    if own >= wall - EPS || a <= EPS {
        return Err(invalid(F, format!("the {what} band ({w} mm) is not narrower than the wall")));
    }
    Ok((outer, hole))
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let (w, h, cl) = (req.param("width")?, req.param("height")?, req.param("clearance")?);
    let inner = req.choice("position")? == "inner";
    let face = req.face("face")?;
    let kind = req.choice("type")?;
    let mut ops = Vec::new();
    let groove = |input: &str, f: &FaceGeom, id: u32| -> Result<Op, PluginError> {
        let (outer, hole) = band(f, inner, w, cl, "groove")?;
        if h + cl >= -f.body_span[0] - EPS {
            return Err(invalid(F, "the groove is deeper than the part"));
        }
        Ok(prism(id, input, f.frame, outer, vec![hole], (-(h + cl), 1.0), 0.0, Mode::Cut, "groove".into()))
    };
    if kind != "groove" {
        let (outer, hole) = band(face, inner, w, 0.0, "lip")?;
        ops.push(prism(0, "face", face.frame, outer, vec![hole], (0.0, h), 0.0, Mode::Join, "lip".into()));
    }
    match kind {
        "groove" => ops.push(groove("face", face, 1)?),
        "lip_and_groove" => {
            let m = req.face("mate").map_err(|_| invalid(F, "lip and groove needs the mating rim face"))?;
            if (rcad_plugin::geom::dot(m.frame.z, face.frame.z) + 1.0).abs() > 1e-9 {
                return Err(invalid(F, "the mating rim must face the rim (parallel, opposite normals)"));
            }
            ops.push(groove("mate", m, 1)?);
        }
        _ => {}
    }
    Ok(ops)
}
