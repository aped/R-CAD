//! `rcad-cli script` and `rcad-cli addin` (docs/SCRIPTING.md) through
//! the built binary: the examples against hand-derived closed-form volumes, unit
//! conversion, unsupported members, script errors, add-in manifests, command listing,
//! inputs, validation and selections, and the Python unit tests of the recorder and the
//! add-in command model.
//!
//! Every test is skipped, with a note on standard error, when `python3` is not installed.
mod common;

use serde_json::Value;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn have_python() -> bool {
    let ok = Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("python3 is not installed: script tests skipped");
    }
    ok
}

fn cli(args: &[&Path]) -> Output {
    Command::new(common::rcad_cli())
        .args(args)
        .env("XDG_DATA_HOME", tmp_data())
        .env_remove("RCAD_PLUGIN_PATH")
        .output()
        .expect("run rcad-cli")
}

/// An empty data directory, so no installed external plugins are loaded.
fn tmp_data() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rcad-pyscript-test-data-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `script SCRIPT --out DOC --commands CMDS` in `dir`.
fn run_script(script: &Path, dir: &Path) -> (Output, PathBuf, PathBuf) {
    let doc = dir.join("doc.rcad.json");
    let cmds = dir.join("commands.json");
    let o = cli(&[
        Path::new("script"),
        script,
        Path::new("--out"),
        &doc,
        Path::new("--commands"),
        &cmds,
    ]);
    (o, doc, cmds)
}

/// Volumes of the bodies of a saved document, by opening it with `rcad-cli run`.
fn volumes(doc: &Path, dir: &Path) -> Vec<f64> {
    let script = dir.join("inspect.json");
    let steps = serde_json::json!([{"cmd": "open", "path": doc}, {"cmd": "inspect"}]);
    std::fs::write(&script, steps.to_string()).unwrap();
    let o = cli(&[Path::new("run"), &script]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let out = text(&o.stdout);
    let last: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    last["response"]["summary"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            b["measurements"]["volume"]["Value"]["volume"]
                .as_f64()
                .unwrap_or_else(|| panic!("no volume: {b}"))
        })
        .collect()
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= 1e-6 * want.abs(),
        "{what}: got {got}, want {want}"
    );
}

/// Run an example, check it succeeded, that its recorded commands replay with `run`,
/// and return the saved body volumes.
fn example(name: &str) -> Vec<f64> {
    let dir = tempfile::tempdir().unwrap();
    let script = repo().join("compat/scripting/examples").join(name);
    let (o, doc, cmds) = run_script(&script, dir.path());
    assert!(
        o.status.success(),
        "{name}: {}\n{}",
        text(&o.stdout),
        text(&o.stderr)
    );
    for line in text(&o.stdout).lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["as_expected"], true, "{line}");
    }
    let replay = cli(&[Path::new("run"), &cmds]);
    assert!(replay.status.success(), "{}", text(&replay.stdout));
    volumes(&doc, dir.path())
}

#[test]
fn plate_with_hole_has_the_closed_form_volume() {
    if !have_python() {
        return;
    }
    // 100 x 60 mm plate, 8 mm (0.8 cm) thick, minus a 7.5 mm radius hole through it.
    let want = 100.0 * 60.0 * 8.0 - PI * 7.5 * 7.5 * 8.0;
    let v = example("plate_with_hole.py");
    assert_eq!(v.len(), 1);
    assert_close(v[0], want, "plate");
}

#[test]
fn plastic_box_has_the_closed_form_volume() {
    if !have_python() {
        return;
    }
    // 80 x 50 x 25 box shelled to 2 mm walls and floor: 80*50*25 - 76*46*23.
    let shell = 80.0 * 50.0 * 25.0 - 76.0 * 46.0 * 23.0;
    // Four bosses on the floor (z = 2), 15 mm high (offset 1.5 cm), 6 mm across, with
    // 2.5 mm pilot holes 6 mm deep.
    let bosses = 4.0 * (PI * 9.0 * 15.0 - PI * 1.25 * 1.25 * 6.0);
    // Web at x = 0 from the rim (z = 25) to the floor (z = 2), 2 mm thick, across the
    // 46 mm cavity.
    let web = 46.0 * 2.0 * 23.0;
    // Rib at y = 0 (XZ plane), 2 mm thick, from z = 15 down to the floor, across the
    // 76 mm cavity, less the 2 x 2 x 13 mm where it crosses the web.
    let rib = 76.0 * 2.0 * 13.0 - 2.0 * 2.0 * 13.0;
    let v = example("plastic_box.py");
    assert_eq!(v.len(), 1);
    assert_close(v[0], shell + bosses + web + rib, "plastic box");
}

fn write_script(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("script.py");
    let header = "import adsk.core, adsk.fusion, adsk.cam, traceback\n\
                  V = adsk.core.ValueInput\nP = adsk.core.Point3D\n";
    std::fs::write(&p, format!("{header}{body}")).unwrap();
    p
}

const BLOCK: &str = "
def block(root, d):
    sk = root.sketches.add(root.xYConstructionPlane)
    sk.sketchCurves.sketchLines.addTwoPointRectangle(P.create(0, 0, 0), P.create(2, 1, 0))
    return root.features.extrudeFeatures.addSimple(
        sk.profiles.item(0), d, adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
";

#[test]
fn reals_are_centimetres_and_strings_carry_units() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(
        dir.path(),
        &format!(
            "{BLOCK}
def run(context):
    root = adsk.core.Application.get().activeProduct.rootComponent
    block(root, V.createByReal(1.5))
    block(root, V.createByString('0.25 in'))
"
        ),
    );
    let (o, doc, cmds) = run_script(&script, dir.path());
    assert!(o.status.success(), "{}", text(&o.stderr));
    let cmds: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(cmds).unwrap()).unwrap();
    let d: Vec<f64> = cmds
        .iter()
        .filter(|c| c["cmd"] == "extrude")
        .map(|c| c["distance"].as_f64().unwrap())
        .collect();
    assert_eq!(d, [15.0, 6.35]);
    // The sketch is 2 x 1 cm = 20 x 10 mm.
    let v = volumes(&doc, dir.path());
    assert_close(v[0], 20.0 * 10.0 * 15.0, "1.5 cm block");
    assert_close(v[1], 20.0 * 10.0 * 6.35, "0.25 in block");
}

#[test]
fn an_unsupported_member_fails_with_its_name() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(
        dir.path(),
        "
def run(context):
    root = adsk.core.Application.get().activeProduct.rootComponent
    root.features.loftFeatures.createInput(adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
",
    );
    let (o, doc, _) = run_script(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(
        err.contains(
            "not supported: adsk.fusion.Features.loftFeatures is not in R-CAD's scripting API subset"
        ),
        "{err}"
    );
    assert!(err.contains("script.py"), "the script line is shown: {err}");
    assert!(o.stdout.is_empty(), "nothing is executed");
    assert!(!doc.exists());
}

#[test]
fn an_unsupported_member_caught_by_the_script_still_fails() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(
        dir.path(),
        &format!(
            "{BLOCK}
def run(context):
    ui = None
    try:
        app = adsk.core.Application.get()
        ui = app.userInterface
        root = app.activeProduct.rootComponent
        block(root, V.createByReal(1))
        root.features.chamferFeatures
    except:
        if ui:
            ui.messageBox('Failed:\\n{{}}'.format(traceback.format_exc()))
"
        ),
    );
    let (o, _, _) = run_script(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(
        err.contains("adsk.fusion.Features.chamferFeatures is not in R-CAD's scripting API subset"),
        "{err}"
    );
    assert!(err.contains("which caught it"), "{err}");
}

#[test]
fn python_errors_and_missing_scripts_are_reported() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = write_script(dir.path(), "def run(context):\n    1 / 0\n");
    let (o, _, _) = run_script(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(err.contains("Python error: ZeroDivisionError"), "{err}");
    assert!(err.contains("Traceback"), "{err}");

    let script = write_script(dir.path(), "x = 1\n");
    let (o, _, _) = run_script(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("no run(context) function"));

    let (o, _, _) = run_script(&dir.path().join("missing.py"), dir.path());
    assert_eq!(o.status.code(), Some(2));
    let o = cli(&[Path::new("script")]);
    assert_eq!(o.status.code(), Some(64));
}

#[test]
fn a_refused_step_stops_the_run() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // A cut pointing away from the top face removes nothing: refused, exit 1.
    let script = write_script(
        dir.path(),
        &format!(
            "{BLOCK}
def run(context):
    root = adsk.core.Application.get().activeProduct.rootComponent
    e = block(root, V.createByReal(1))
    sk = root.sketches.add(e.endFaces.item(0))
    sk.sketchCurves.sketchCircles.addByCenterRadius(P.create(1, 0.5, 0), 0.2)
    root.features.extrudeFeatures.addSimple(
        sk.profiles.item(0), V.createByReal(0.5), adsk.fusion.FeatureOperations.CutFeatureOperation)
"
        ),
    );
    let (o, doc, cmds) = run_script(&script, dir.path());
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stderr).contains("extrude was refused"),
        "{}",
        text(&o.stderr)
    );
    assert!(!doc.exists(), "nothing is saved after a refusal");
    assert!(cmds.exists(), "the recorded commands are still written");
}

#[test]
fn recorder_unit_tests_pass() {
    if !have_python() {
        return;
    }
    let o = Command::new("python3")
        .args(["-I", "-m", "unittest", "discover", "-s"])
        .arg(repo().join("compat/scripting/tests"))
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{}\n{}",
        text(&o.stdout),
        text(&o.stderr)
    );
}

// ---------------------------------------------------------------------------
// addin

fn plastic_box_addin() -> PathBuf {
    repo().join("compat/scripting/examples/addins/PlasticBoxAddin")
}

/// `addin DIR ARGS... --out DOC --commands CMDS` in `dir`.
fn addin(addin_dir: &Path, args: &[&str], dir: &Path) -> (Output, PathBuf, PathBuf) {
    let doc = dir.join("doc.rcad.json");
    let cmds = dir.join("commands.json");
    let mut all: Vec<&Path> = vec![Path::new("addin"), addin_dir];
    all.extend(args.iter().map(Path::new));
    all.extend([Path::new("--out"), &doc, Path::new("--commands"), &cmds]);
    let o = cli(&all);
    (o, doc, cmds)
}

/// Run an add-in command, check every step ran as recorded and that the recorded
/// commands replay with `run`; return the body volumes and the commands.
fn addin_volumes(addin_dir: &Path, args: &[&str]) -> (Vec<f64>, Vec<Value>) {
    let dir = tempfile::tempdir().unwrap();
    let (o, doc, cmds) = addin(addin_dir, args, dir.path());
    assert!(
        o.status.success(),
        "{args:?}: {}\n{}",
        text(&o.stdout),
        text(&o.stderr)
    );
    for line in text(&o.stdout).lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["as_expected"], true, "{line}");
    }
    let replay = cli(&[Path::new("run"), &cmds]);
    assert!(replay.status.success(), "{}", text(&replay.stdout));
    let commands: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&cmds).unwrap()).unwrap();
    (volumes(&doc, dir.path()), commands)
}

/// The material of the last plastic rule stored by the commands.
fn rule_material(commands: &[Value]) -> String {
    let data = commands
        .iter()
        .rfind(|c| c["cmd"] == "set_plugin_data")
        .expect("the rule is stored");
    data["data"]["rules"][0]["material"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A boss 6 mm across and `height` tall with a 2.5 mm pilot hole 6 mm deep.
fn boss(height: f64) -> f64 {
    PI * 9.0 * height - PI * 1.25 * 1.25 * 6.0
}

#[test]
fn addin_list_shows_the_commands_and_their_inputs() {
    if !have_python() {
        return;
    }
    let o = cli(&[
        Path::new("addin"),
        &plastic_box_addin(),
        Path::new("--list"),
    ]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["addin"], "PlasticBoxAddin");
    assert_eq!(v["manifest"]["version"], "1.0.0");
    assert_eq!(v["manifest"]["runOnStartup"], false);
    let cmds = v["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1);
    let c = &cmds[0];
    assert_eq!(c["id"], "PlasticBoxCommand");
    assert_eq!(c["name"], "Plastic box");
    assert_eq!(
        c["placements"],
        serde_json::json!([{"workspace": "FusionSolidEnvironment",
                            "panel": "SolidScriptsAddinsPanel", "promoted": true}])
    );
    let input = |id: &str| {
        c["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .unwrap_or_else(|| panic!("no input {id}: {c}"))
            .clone()
    };
    // Value inputs: the expression as typed, the value in internal units (cm).
    assert_eq!(input("width")["type"], "ValueCommandInput");
    assert_eq!(input("width")["expression"], "80 mm");
    assert_eq!(input("width")["value"], 8.0);
    assert_eq!(input("wall")["expression"], "2 mm");
    assert_eq!(input("wall")["value"], 0.2);
    assert_eq!(input("bosses")["type"], "IntegerSpinnerCommandInput");
    assert_eq!(input("bosses")["value"], 2);
    assert_eq!(input("bosses")["maximum"], 6);
    assert_eq!(input("web")["value"], true);
    assert_eq!(
        input("rule")["items"],
        serde_json::json!(["ABS", "PC", "PP", "PA66"])
    );
    assert_eq!(input("rule")["selected"], "ABS");
}

#[test]
fn addin_with_default_inputs_has_the_closed_form_volume() {
    if !have_python() {
        return;
    }
    // 80 x 50 x 25 box shelled to 2 mm; 2 x 2 bosses on the floor (z = 2) with tops
    // 5 mm below the rim (18 mm tall); a 2 mm web along the middle from the rim to the
    // floor across the 76 mm cavity.
    let want = 80.0 * 50.0 * 25.0 - 76.0 * 46.0 * 23.0 + 4.0 * boss(18.0) + 76.0 * 2.0 * 23.0;
    let (v, commands) = addin_volumes(&plastic_box_addin(), &[]);
    assert_eq!(v.len(), 1);
    assert_close(v[0], want, "default plastic box");
    assert_eq!(rule_material(&commands), "abs");
    // The preview built the same box; it was discarded, so the box is built once.
    let extrudes = commands.iter().filter(|c| c["cmd"] == "extrude").count();
    assert_eq!(extrudes, 1);
}

#[test]
fn addin_inputs_override_the_defaults() {
    if !have_python() {
        return;
    }
    // 100 x 60 x 25 box, 2.5 mm walls (a plain number is in the input's mm), 3 x 2
    // bosses 25 - 2.5 - 5 = 17.5 mm tall, no web, the PP preset.
    let want = 100.0 * 60.0 * 25.0 - 95.0 * 55.0 * 22.5 + 6.0 * boss(17.5);
    let (v, commands) = addin_volumes(
        &plastic_box_addin(),
        &[
            "--input",
            "width=100 mm",
            "--input",
            "depth=6 cm",
            "--input",
            "wall=2.5",
            "--input",
            "bosses=3",
            "--input",
            "web=false",
            "--input",
            "rule=PP",
        ],
    );
    assert_eq!(v.len(), 1);
    assert_close(v[0], want, "overridden plastic box");
    assert_eq!(rule_material(&commands), "pp");
    assert!(
        !commands
            .iter()
            .any(|c| c["cmd"] == "plugin_feature" && c["feature"] == "web"),
        "no web"
    );
}

#[test]
fn addin_inputs_the_addin_rejects_are_refused() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // 30 mm walls meet in a 50 mm deep box: the validateInputs handler says no.
    let (o, doc, _) = addin(&plastic_box_addin(), &["--input", "wall=30 mm"], dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(err.contains("areInputsValid = False"), "{err}");
    assert!(o.stdout.is_empty(), "nothing is executed");
    assert!(!doc.exists());

    for (arg, msg) in [
        ("bosses=9", "input 'bosses': 9 is outside 0..6"),
        ("rule=Steel", "'Steel' is not an item"),
        ("height=3 deg", "input 'height'"),
        ("colour=red", "has no input 'colour'"),
    ] {
        let (o, _, _) = addin(&plastic_box_addin(), &["--input", arg], dir.path());
        assert_eq!(o.status.code(), Some(1), "{arg}");
        let err = text(&o.stderr);
        assert!(err.contains(msg), "{arg}: {err}");
    }
}

/// An add-in folder `Name` with `manifest` and `Name.py`.
fn write_addin(dir: &Path, name: &str, manifest: &str, py: &str) -> PathBuf {
    let d = dir.join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join(format!("{name}.manifest")), manifest).unwrap();
    std::fs::write(d.join(format!("{name}.py")), py).unwrap();
    d
}

const ADDIN_MANIFEST: &str = r#"{"autodeskProduct": "Fusion", "type": "addin", "id": "x",
    "author": "t", "description": {"": "test"}, "version": "1", "runOnStartup": false}"#;

/// An add-in whose one command runs `body` in its execute handler (`root`, `inputs`, `V`,
/// `P`, `F` and `block(size_cm, height_cm[, op, plane])` are defined).
fn handler_addin(body: &str, created: &str) -> String {
    format!(
        "import adsk.core, adsk.fusion, traceback
V = adsk.core.ValueInput
P = adsk.core.Point3D
F = adsk.fusion.FeatureOperations
_handlers = []

def block(size, height, op=F.NewBodyFeatureOperation, plane=None):
    root = adsk.core.Application.get().activeProduct.rootComponent
    sk = root.sketches.add(plane or root.xYConstructionPlane)
    sk.sketchCurves.sketchLines.addTwoPointRectangle(P.create(0, 0, 0), P.create(size, size, 0))
    return root.features.extrudeFeatures.addSimple(sk.profiles.item(0), V.createByReal(height), op)

class Execute(adsk.core.CommandEventHandler):
    def notify(self, args):
        inputs = args.command.commandInputs
        root = adsk.core.Application.get().activeProduct.rootComponent
        {body}

class Created(adsk.core.CommandCreatedEventHandler):
    def notify(self, args):
        inputs = args.command.commandInputs
        {created}
        h = Execute()
        args.command.execute.add(h)
        _handlers.append(h)

def run(context):
    ui = adsk.core.Application.get().userInterface
    d = ui.commandDefinitions.addButtonDefinition('Cmd', 'Cmd', '')
    h = Created()
    d.commandCreated.add(h)
    _handlers.append(h)
"
    )
}

#[test]
fn a_manifest_that_is_not_an_addin_is_refused() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let py = handler_addin("block(1, 1)", "pass");
    let script = write_addin(
        dir.path(),
        "Script",
        r#"{"autodeskProduct": "Fusion", "type": "script", "id": "x"}"#,
        &py,
    );
    let (o, _, _) = addin(&script, &[], dir.path());
    assert_eq!(o.status.code(), Some(2));
    let err = text(&o.stderr);
    assert!(
        err.contains("describes a script, not an add-in") && err.contains("script"),
        "{err}"
    );

    let untyped = write_addin(dir.path(), "Untyped", r#"{"id": "x"}"#, &py);
    let (o, _, _) = addin(&untyped, &[], dir.path());
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("has no \"type\": \"addin\""));

    let bad = write_addin(dir.path(), "Bad", "{not json", &py);
    let (o, _, _) = addin(&bad, &[], dir.path());
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("is not valid JSON"));

    let empty = dir.path().join("Empty");
    std::fs::create_dir_all(&empty).unwrap();
    let (o, _, _) = addin(&empty, &[], dir.path());
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o.stderr).contains("has no Empty.manifest"));

    let o = cli(&[Path::new("addin")]);
    assert_eq!(o.status.code(), Some(64));
    let o = cli(&[Path::new("addin"), &empty, Path::new("--input")]);
    assert_eq!(o.status.code(), Some(64));
}

#[test]
fn an_addin_using_an_unsupported_member_fails_naming_it() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let d = write_addin(
        dir.path(),
        "Loft",
        ADDIN_MANIFEST,
        &handler_addin(
            "root.features.loftFeatures.createInput(F.NewBodyFeatureOperation)",
            "pass",
        ),
    );
    let (o, doc, _) = addin(&d, &[], dir.path());
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(
        err.contains(
            "addin: not supported: adsk.fusion.Features.loftFeatures is not in R-CAD's \
             scripting API subset"
        ),
        "{err}"
    );
    assert!(err.contains("Loft.py"), "the add-in line is shown: {err}");
    assert!(!doc.exists());

    // An unsupported input kind, in the commandCreated handler.
    let d = write_addin(
        dir.path(),
        "Table",
        ADDIN_MANIFEST,
        &handler_addin("pass", "inputs.addTableCommandInput('t', 'T', 2, '1:1')"),
    );
    let o = cli(&[Path::new("addin"), &d, Path::new("--list")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stderr).contains("adsk.core.CommandInputs.addTableCommandInput"),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn an_addin_selection_names_a_face_of_the_base_script() {
    if !have_python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // The base script: a 30 x 30 x 10 mm block.
    let base = write_script(
        dir.path(),
        &format!(
            "{BLOCK}
def run(context):
    root = adsk.core.Application.get().activeProduct.rootComponent
    sk = root.sketches.add(root.xYConstructionPlane)
    sk.sketchCurves.sketchLines.addTwoPointRectangle(P.create(0, 0, 0), P.create(3, 3, 0))
    root.features.extrudeFeatures.addSimple(
        sk.profiles.item(0), V.createByReal(1), adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
"
        ),
    );
    // The add-in: a 10 x 10 mm block of `height` joined onto the selected face.
    let d = write_addin(
        dir.path(),
        "Stack",
        ADDIN_MANIFEST,
        &handler_addin(
            "block(1, inputs.itemById('height').value, F.JoinFeatureOperation, \
             inputs.itemById('face').selection(0).entity)",
            "s = inputs.addSelectionInput('face', 'Face', 'Pick a face')
        s.addSelectionFilter('PlanarFaces')
        inputs.addValueInput('height', 'Height', 'mm', V.createByString('5 mm'))",
        ),
    );
    let base_arg = base.to_string_lossy().into_owned();
    let (v, _) = addin_volumes(
        &d,
        &[
            "--base",
            &base_arg,
            "--input",
            "face=face:1:endFaces",
            "--input",
            "height=0.25 in",
        ],
    );
    assert_eq!(v.len(), 1);
    assert_close(
        v[0],
        30.0 * 30.0 * 10.0 + 10.0 * 10.0 * 6.35,
        "block with a block on top",
    );

    // Without a selection the command cannot run.
    let (o, _, _) = addin(&d, &["--base", &base_arg], dir.path());
    assert_eq!(o.status.code(), Some(1));
    assert!(
        text(&o.stderr).contains("needs at least 1 selection"),
        "{}",
        text(&o.stderr)
    );
}
