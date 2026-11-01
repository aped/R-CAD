//! `rcad-cli addin ADDIN_DIR [--list] [--command ID] [--input ID=VALUE ...]
//! [--base SCRIPT.py] [--out DOC.rcad.json] [--commands OUT.json]` (docs/SCRIPTING.md,
//! "Add-ins").
//!
//! Runs an add-in folder written for a subset of the add-in scripting API without a user
//! interface: the manifest (`<Name>.manifest`, `type` `addin`) is checked here, then the
//! bootstrap (`adsk._addin`) imports `<Name>.py`, calls its `run(context)` and plays the
//! user's part for one command: `commandCreated`, the `--input` values (each firing
//! `inputChanged`), `validateInputs`, a discarded `executePreview`, `execute`, `destroy`
//! and the add-in's `stop(context)`. The recorded commands then run as for
//! `script`. `--list` prints the registered commands and their inputs instead.
//!
//! Exit status: 0 success; 1 the add-in failed (Python error, unsupported member, an input
//! it cannot take, inputs its validateInputs handler rejected) or a step was refused; 2 the
//! folder, manifest or Python file cannot be read, the manifest is not an add-in's, Python
//! is missing, or an output cannot be written; 64 usage.
use crate::out::{esay, say};
use crate::pyscript::{Bootstrap, execute};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: rcad-cli addin ADDIN_DIR [--list] [--command ID] \
                     [--input ID=VALUE ...] [--base SCRIPT.py] [--out DOC.rcad.json] \
                     [--commands OUT.json]";

fn usage() -> ExitCode {
    esay!("{USAGE}");
    ExitCode::from(64)
}

#[derive(Default)]
struct Args {
    dir: Option<PathBuf>,
    list: bool,
    command: Option<String>,
    inputs: Vec<(String, String)>,
    base: Option<PathBuf>,
    out: Option<PathBuf>,
    commands: Option<PathBuf>,
}

fn parse(args: &[String]) -> Option<Args> {
    let mut a = Args::default();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--list" => {
                a.list = true;
                i += 1;
            }
            "--command" | "--input" | "--base" | "--out" | "--commands" => {
                let v = args.get(i + 1)?;
                match flag {
                    "--command" => a.command = Some(v.clone()),
                    "--input" => {
                        let (id, value) = v.split_once('=')?;
                        if id.is_empty() {
                            return None;
                        }
                        a.inputs.push((id.to_owned(), value.to_owned()));
                    }
                    "--base" => a.base = Some(PathBuf::from(v)),
                    "--out" => a.out = Some(PathBuf::from(v)),
                    _ => a.commands = Some(PathBuf::from(v)),
                }
                i += 2;
            }
            f if f.starts_with('-') => return None,
            d => {
                if a.dir.replace(PathBuf::from(d)).is_some() {
                    return None;
                }
                i += 1;
            }
        }
    }
    a.dir.as_ref()?;
    if a.list
        && (a.command.is_some() || !a.inputs.is_empty() || a.out.is_some() || a.commands.is_some())
    {
        return None;
    }
    Some(a)
}

/// A checked add-in folder: its Python file and manifest.
struct Addin {
    dir: PathBuf,
    script: PathBuf,
    manifest: Value,
}

/// Find and check `<Name>.manifest` and `<Name>.py` in `dir`.
fn load(dir: &Path) -> Result<Addin, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let name = dir
        .canonicalize()
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let mut manifest_path = dir.join(format!("{name}.manifest"));
    if !manifest_path.is_file() {
        // The manifest is normally named after the folder; accept a single other one.
        let found: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension() == Some(OsStr::new("manifest")) && p.is_file())
            .collect();
        match found.as_slice() {
            [one] => manifest_path = one.clone(),
            [] => {
                return Err(format!(
                    "{} has no {name}.manifest: an add-in folder holds <Name>.manifest and \
                     <Name>.py",
                    dir.display()
                ));
            }
            _ => {
                return Err(format!(
                    "{} has several .manifest files and none named {name}.manifest",
                    dir.display()
                ));
            }
        }
    }
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{} is not valid JSON: {e}", manifest_path.display()))?;
    let Some(obj) = manifest.as_object() else {
        return Err(format!("{} is not a JSON object", manifest_path.display()));
    };
    match obj.get("type").and_then(Value::as_str) {
        Some(t) if t.eq_ignore_ascii_case("addin") => {}
        Some(t) if t.eq_ignore_ascii_case("script") => {
            return Err(format!(
                "{} describes a script, not an add-in; run its .py with rcad-cli script",
                manifest_path.display()
            ));
        }
        Some(t) => {
            return Err(format!(
                "{}: type {t:?} is not \"addin\"",
                manifest_path.display()
            ));
        }
        None => {
            return Err(format!(
                "{} has no \"type\": \"addin\"",
                manifest_path.display()
            ));
        }
    }
    for key in ["id", "author", "version", "autodeskProduct", "supportedOS"] {
        if let Some(v) = obj.get(key)
            && !v.is_string()
        {
            return Err(format!(
                "{}: {key} must be a string",
                manifest_path.display()
            ));
        }
    }
    if let Some(v) = obj.get("runOnStartup")
        && !v.is_boolean()
    {
        return Err(format!(
            "{}: runOnStartup must be true or false",
            manifest_path.display()
        ));
    }
    let stem = manifest_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let script = dir.join(format!("{stem}.py"));
    if !script.is_file() {
        let native = ["dll", "dylib", "so"]
            .iter()
            .any(|ext| dir.join(format!("{stem}.{ext}")).is_file());
        return Err(if native {
            format!(
                "{} is a C++ add-in; only Python add-ins are supported",
                dir.display()
            )
        } else {
            format!("{} has no {stem}.py", dir.display())
        });
    }
    Ok(Addin {
        dir: dir.to_path_buf(),
        script,
        manifest,
    })
}

/// `addin` with its arguments (after the word `addin`).
pub fn run(args: &[String]) -> ExitCode {
    let Some(a) = parse(args) else {
        return usage();
    };
    let Some(dir) = a.dir.as_deref() else {
        return usage();
    };
    let addin = match load(dir) {
        Ok(x) => x,
        Err(e) => {
            esay!("addin: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some(b) = &a.base
        && !b.is_file()
    {
        esay!("cannot read {}: not a file", b.display());
        return ExitCode::from(2);
    }
    let abs = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let request = json!({
        "addin": abs(&addin.dir),
        "script": abs(&addin.script),
        "list": a.list,
        "command": a.command,
        "inputs": a.inputs.iter().map(|(k, v)| [k, v]).collect::<Vec<_>>(),
        "base": a.base.as_deref().map(abs),
    });
    let boot = match Bootstrap::new() {
        Ok(b) => b,
        Err(code) => return code,
    };
    let request_path = boot.dir().join("addin-request.json");
    if let Err(e) = std::fs::write(&request_path, request.to_string()) {
        esay!("cannot write {}: {e}", request_path.display());
        return ExitCode::from(2);
    }
    let recorded = match boot.run(
        "addin",
        &[OsStr::new("--addin"), request_path.as_os_str()],
        &addin.script,
    ) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if a.list {
        let commands = recorded.listing.unwrap_or(Value::Array(Vec::new()));
        let m = &addin.manifest;
        let listing = json!({
            "addin": addin.script.file_stem().map(|s| s.to_string_lossy()),
            "manifest": {
                "id": m.get("id"),
                "author": m.get("author"),
                "description": m.get("description"),
                "version": m.get("version"),
                "runOnStartup": m.get("runOnStartup"),
                "supportedOS": m.get("supportedOS"),
            },
            "commands": commands,
        });
        match serde_json::to_string_pretty(&listing) {
            Ok(t) => say!("{t}"),
            Err(e) => {
                esay!("cannot serialize the listing: {e}");
                return ExitCode::from(2);
            }
        }
        return ExitCode::SUCCESS;
    }
    if let Some(info) = &recorded.addin {
        esay!(
            "addin: ran command {} (preview: {})",
            info["command"],
            info["preview"].as_str().unwrap_or("none")
        );
    }
    crate::load_plugins();
    execute(
        "addin",
        recorded.steps,
        a.out.as_deref(),
        a.commands.as_deref(),
    )
}
