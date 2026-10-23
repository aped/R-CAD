//! Draft (docs/PLASTICS.md "Draft"): Fusion's fixed-plane draft of existing
//! faces. Each selected planar face is turned about its line on the fixed
//! plane by `draft_deg` (relative to its current orientation), leaning in as
//! it goes along the pull direction (away from the fixed face, into the
//! part; `reverse`: the other way). Built with the kernel's exact face
//! replacement, which keeps topology (one operation per face, id = its
//! position in the selection).
use crate::util::invalid;
use crate::{choice, input, param};
use rcad_plugin::geom::{add, cross, dot, norm, scale, sub};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, InputKind, Op, ParamKind, PluginError};

const F: &str = "draft";

pub fn schema() -> FeatureSchema {
    FeatureSchema {
        id: F.into(),
        name: "Draft".into(),
        description: "Fixed-plane draft of planar faces (relative angle)".into(),
        inputs: vec![
            input("faces", "Faces to draft", InputKind::Faces, false),
            input("fixed", "Fixed plane (face)", InputKind::Face, false),
        ],
        params: vec![param(
            "draft_deg",
            "Draft angle",
            ParamKind::Angle,
            1.0,
            Some((0.0, true)),
            Some(30.0),
            Some("DA"),
        )],
        choices: vec![choice("direction", "Direction", &["pull", "reverse"])],
    }
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    scale(v, 1.0 / norm(v))
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let a = req.param("draft_deg")?.to_radians();
    let sign = if req.choice("direction")? == "reverse" {
        -1.0
    } else {
        1.0
    };
    let fixed = req.face("fixed")?;
    let faces = req.faces("faces")?;
    if faces.is_empty() {
        return Err(invalid(F, "select at least one face"));
    }
    let nf = fixed.frame.z;
    // Pull: away from the fixed face, into the part.
    let pull = scale(nf, -1.0);
    let mut ops = Vec::new();
    for (i, f) in faces.iter().enumerate() {
        if f.body != fixed.body {
            return Err(invalid(
                F,
                format!("face {} is on another body than the fixed plane", i + 1),
            ));
        }
        let nw = f.frame.z;
        let l = cross(nw, nf);
        if norm(l) < 1e-9 {
            return Err(invalid(
                F,
                format!("face {} is parallel to the fixed plane", i + 1),
            ));
        }
        let l = unit(l);
        // A point on both planes (the hinge line).
        let (d1, d2) = (dot(nw, f.frame.origin), dot(nf, fixed.frame.origin));
        let ll = dot(cross(nw, nf), cross(nw, nf));
        let p0 = scale(
            add(
                scale(cross(nf, cross(nw, nf)), d1),
                scale(cross(cross(nw, nf), nw), d2),
            ),
            1.0 / ll,
        );
        // Turn the normal about the hinge towards the pull (the face leans
        // in going along it).
        let mut t = cross(l, nw);
        if dot(t, pull) < 0.0 {
            t = scale(t, -1.0);
        }
        let n = unit(add(scale(nw, a.cos()), scale(t, sign * a.sin())));
        let _ = sub;
        ops.push(Op::ReplaceFace {
            id: i as u32,
            face: "faces".into(),
            index: i as u32,
            origin: p0,
            normal: n,
            label: format!("draft face {}", i + 1),
        });
    }
    Ok(ops)
}
