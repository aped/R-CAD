//! Out-of-process plugins: a directory holding `plugin.json` (a
//! [`Manifest`] plus a `command`) and whatever the command runs. Each call
//! starts the command in the plugin directory, writes one JSON request
//! (followed by a newline) to its standard input, closes it, and reads one
//! JSON answer from its standard output until the program closes it:
//!
//! ```text
//! -> {"method": "build",    "request": BuildRequest}
//! <- {"ok": [Op, ...]}            or {"error": "message"}
//! -> {"method": "analyze",  "request": AnalysisRequest}
//! <- {"ok": <report JSON>}        or {"error": "message"}
//! -> {"method": "validate_data", "data": <JSON>}
//! <- {"ok": null}                 or {"error": "message"}
//! -> {"method": "data_effects", "data": <JSON>}
//! <- {"ok": DataEffects}
//! -> {"method": "rule_symbols", "data": <JSON>, "body": 3}
//! <- {"ok": {"T": "ABS_T", ...}}
//! ```
//!
//! An answer carrying `"error"` (a string) is a stated refusal
//! ([`PluginError::Unsupported`]); anything else that is not a well-formed
//! `"ok"` answer, a non-zero exit, a crash, a timeout or an oversized answer
//! is a [`PluginError::Failed`] that quotes the program's standard error
//! (first [`MAX_STDERR_BYTES`]).
//!
//! The program runs with a cleared environment (only `PATH`, `HOME`, the
//! locale and temporary-directory variables are kept) plus
//! `RCAD_PLUGIN_API` (the interface version) and `RCAD_PLUGIN_DIR` (the
//! plugin directory). On Unix it runs in its own process group, and a call
//! that times out or overflows kills the whole group, so helper processes
//! the plugin started do not outlive the call.
//!
//! The answer is untrusted: it is size-limited, time-limited, parsed into
//! typed values and checked ([`crate::check_ops`]) before the document uses
//! it. Documents never carry plugin code; only installed plugins run.
use crate::{AnalysisRequest, BuildRequest, Manifest, Op, Plugin, PluginError};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Largest accepted answer.
pub const MAX_ANSWER_BYTES: u64 = 64 * 1024 * 1024;
/// Largest accepted `plugin.json`.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Default time limit of one call.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Standard error kept for diagnostics (the rest is read and discarded so the
/// program never blocks on it).
pub const MAX_STDERR_BYTES: usize = 16 * 1024;
/// Longest plugin error message passed on (longer ones are cut).
pub const MAX_ERROR_CHARS: usize = 4096;
/// Most entries of `command`.
const MAX_COMMAND_ARGS: usize = 256;
/// Longest entry of `command`.
const MAX_COMMAND_ARG_BYTES: usize = 4096;
/// How long a finished call waits for standard error to close after the
/// program exited and its answer was read.
const STDERR_GRACE: Duration = Duration::from_millis(200);
/// How long a killed call waits for its pipe threads before leaving them to
/// finish on their own.
const KILL_GRACE: Duration = Duration::from_millis(500);

/// Environment variables passed through to plugin programs.
#[cfg(not(windows))]
const KEPT_ENV: &[&str] = &["PATH", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "TMPDIR"];
#[cfg(windows)]
const KEPT_ENV: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    #[serde(flatten)]
    manifest: Manifest,
    /// Program and arguments, run in the plugin directory. A relative
    /// program path with more than one component (`./plugin.py`,
    /// `bin/plugin`) is resolved against that directory; a bare name
    /// (`python3`) is looked up on `PATH`. No entry may contain a `..`
    /// component.
    command: Vec<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// A plugin served by an external program.
#[derive(Debug)]
pub struct ProcessPlugin {
    manifest: Manifest,
    dir: PathBuf,
    /// The program, resolved against `dir` when it is a relative path.
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
}

/// `ok` present (even `null`) is `Some`.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default, deserialize_with = "present")]
    ok: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

/// Check one `command` entry: bounded, no NUL, no `..` component.
fn check_command_entry(s: &str) -> Result<(), String> {
    if s.len() > MAX_COMMAND_ARG_BYTES {
        return Err(format!(
            "command entry longer than {MAX_COMMAND_ARG_BYTES} bytes"
        ));
    }
    if s.contains('\0') {
        return Err("command entry contains a NUL character".into());
    }
    if Path::new(s).components().any(|c| c == Component::ParentDir) {
        return Err(format!(
            "command entry '{s}' refers outside the plugin directory ('..')"
        ));
    }
    Ok(())
}

impl ProcessPlugin {
    /// Load the plugin in directory `dir` (reads and validates `plugin.json`;
    /// runs nothing). [`ProcessPlugin::dir`] is the canonical path of `dir`.
    pub fn load(dir: &Path) -> Result<ProcessPlugin, PluginError> {
        let path = dir.join("plugin.json");
        let bad = |m: String| PluginError::Manifest(format!("{}: {m}", path.display()));
        let dir = std::fs::canonicalize(dir).map_err(|e| bad(e.to_string()))?;
        if !dir.is_dir() {
            return Err(bad("plugin location is not a directory".into()));
        }
        let meta = std::fs::metadata(&path).map_err(|e| bad(e.to_string()))?;
        if !meta.is_file() {
            return Err(bad("not a regular file".into()));
        }
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err(bad(format!("larger than {MAX_MANIFEST_BYTES} bytes")));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .and_then(|f| f.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|e| bad(e.to_string()))?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(bad(format!("larger than {MAX_MANIFEST_BYTES} bytes")));
        }
        let f: ManifestFile = serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
        f.manifest.validate()?;
        let Some(first) = f.command.first() else {
            return Err(bad("command is empty".into()));
        };
        if first.is_empty() {
            return Err(bad("command names no program".into()));
        }
        if f.command.len() > MAX_COMMAND_ARGS {
            return Err(bad(format!(
                "command has more than {MAX_COMMAND_ARGS} entries"
            )));
        }
        for c in &f.command {
            check_command_entry(c).map_err(bad)?;
        }
        let mut program = PathBuf::from(first);
        if program.is_relative() && program.components().count() > 1 {
            program = dir.join(program);
            if !program.is_file() {
                return Err(bad(format!(
                    "program {} does not exist in the plugin directory",
                    program.display()
                )));
            }
        }
        let timeout = match f.timeout_secs {
            Some(s) if (1..=600).contains(&s) => Duration::from_secs(s),
            Some(s) => return Err(bad(format!("timeout_secs {s} must be 1..=600"))),
            None => DEFAULT_TIMEOUT,
        };
        Ok(ProcessPlugin {
            manifest: f.manifest,
            dir,
            program,
            args: f.command[1..].to_vec(),
            timeout,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Time limit of one call.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Run the program once with `request`; the `ok` value of its answer.
    fn call(&self, request: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        let id = &self.manifest.id;
        let fail = |m: String| PluginError::Failed(format!("{id}: {m}"));
        let mut body = serde_json::to_vec(request).map_err(|e| fail(e.to_string()))?;
        body.push(b'\n');

        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        for k in KEPT_ENV {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        cmd.env("RCAD_PLUGIN_API", crate::API_VERSION.to_string())
            .env("RCAD_PLUGIN_DIR", &self.dir);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd
            .spawn()
            .map_err(|e| fail(format!("cannot start {}: {e}", self.program.display())))?;
        // From here on the guard kills and reaps the program on every exit
        // path (including a panic).
        let mut child = ChildGuard::new(child);
        let (tx, rx) = mpsc::channel::<Event>();
        let mut threads = Vec::with_capacity(3);

        let mut stdin = child.take_stdin().ok_or_else(|| fail("no stdin".into()))?;
        let t = tx.clone();
        threads.push(std::thread::spawn(move || {
            // A program that exits without reading its input makes this fail
            // with a broken pipe; that is not an error of the call (its
            // answer or exit status decides).
            let _ = stdin.write_all(&body);
            drop(stdin);
            let _ = t.send(Event::Written);
        }));
        let mut stdout = child
            .take_stdout()
            .ok_or_else(|| fail("no stdout".into()))?;
        let t = tx.clone();
        threads.push(std::thread::spawn(move || {
            let _ = t.send(Event::Stdout(read_capped(&mut stdout)));
        }));
        let mut stderr = child
            .take_stderr()
            .ok_or_else(|| fail("no stderr".into()))?;
        let t = tx;
        threads.push(std::thread::spawn(move || {
            let _ = t.send(Event::Stderr(drain_keeping(&mut stderr, MAX_STDERR_BYTES)));
        }));
        let mut pipes = Pipes {
            rx,
            threads,
            out: None,
            err: None,
        };

        let deadline = Instant::now() + self.timeout;
        let mut status: Option<ExitStatus> = None;
        let mut rest_done: Option<Instant> = None;
        loop {
            if let Some(Stdout::TooLarge) = pipes.out {
                child.kill();
                pipes.settle(KILL_GRACE);
                return Err(fail(format!("answer larger than {MAX_ANSWER_BYTES} bytes")));
            }
            if status.is_none() {
                status = child.try_wait().map_err(|e| fail(e.to_string()))?;
            }
            if status.is_some() && pipes.out.is_some() {
                if pipes.err.is_some() {
                    break;
                }
                // A helper process may still hold standard error open; do not
                // wait for it long once the answer is complete.
                let t = *rest_done.get_or_insert_with(Instant::now);
                if t.elapsed() >= STDERR_GRACE {
                    break;
                }
            }
            let now = Instant::now();
            if now >= deadline {
                let alive = status.is_none();
                child.kill();
                pipes.settle(KILL_GRACE);
                let what = if alive {
                    "no answer"
                } else {
                    "the program exited but its output stayed open (a child process holds it)"
                };
                let err = pipes.stderr_text();
                return Err(fail(format!(
                    "{what} within {} s{}",
                    self.timeout.as_secs(),
                    if err.is_empty() {
                        String::new()
                    } else {
                        format!(": {err}")
                    }
                )));
            }
            let wait = (deadline - now).min(Duration::from_millis(5));
            pipes.poll(wait);
        }
        // The program has been reaped; collect the threads that finished.
        pipes.settle(Duration::ZERO);
        let Some(status) = status else {
            return Err(fail("no exit status".into()));
        };
        let out = match pipes.out.take() {
            Some(Stdout::Data(d)) => d,
            Some(Stdout::TooLarge) => {
                return Err(fail(format!("answer larger than {MAX_ANSWER_BYTES} bytes")));
            }
            Some(Stdout::Error(e)) => return Err(fail(format!("reading the answer: {e}"))),
            None => return Err(fail("no answer".into())),
        };
        let err = pipes.stderr_text();
        if !status.success() {
            return Err(fail(format!(
                "exited with {status}{}",
                if err.is_empty() {
                    String::new()
                } else {
                    format!(": {err}")
                }
            )));
        }
        parse_answer(id, &out, &err)
    }
}

/// Interpret the bytes of an answer.
fn parse_answer(id: &str, out: &[u8], stderr: &str) -> Result<serde_json::Value, PluginError> {
    let fail = |m: String| {
        PluginError::Failed(if stderr.is_empty() {
            format!("{id}: {m}")
        } else {
            format!("{id}: {m} (stderr: {stderr})")
        })
    };
    if out.iter().all(u8::is_ascii_whitespace) {
        return Err(fail("empty answer".into()));
    }
    let a: Answer =
        serde_json::from_slice(out).map_err(|e| fail(format!("answer is not valid: {e}")))?;
    match (a.ok, a.error) {
        (_, Some(e)) => Err(PluginError::Unsupported(format!(
            "{id}: {}",
            clean_text(&e)
        ))),
        (Some(v), None) => Ok(v),
        (None, None) => Err(fail("answer has neither \"ok\" nor \"error\"".into())),
    }
}

/// Text from a plugin made safe to show: control characters other than
/// newline and tab replaced, cut to [`MAX_ERROR_CHARS`].
fn clean_text(s: &str) -> String {
    let mut out: String = s
        .trim()
        .chars()
        .take(MAX_ERROR_CHARS)
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect();
    if s.trim().chars().count() > MAX_ERROR_CHARS {
        out.push('…');
    }
    out
}

enum Stdout {
    Data(Vec<u8>),
    TooLarge,
    Error(std::io::Error),
}

enum Event {
    Written,
    Stdout(Stdout),
    Stderr(Vec<u8>),
}

/// Read until end of file, stopping as soon as more than
/// [`MAX_ANSWER_BYTES`] arrived.
fn read_capped(r: &mut impl Read) -> Stdout {
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => return Stdout::Data(buf),
            Ok(n) => {
                if (buf.len() + n) as u64 > MAX_ANSWER_BYTES {
                    return Stdout::TooLarge;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Stdout::Error(e),
        }
    }
}

/// Read until end of file, keeping the first `keep` bytes.
fn drain_keeping(r: &mut impl Read, keep: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8 * 1024];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => return buf,
            Ok(n) => {
                let room = keep.saturating_sub(buf.len());
                buf.extend_from_slice(&chunk[..n.min(room)]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return buf,
        }
    }
}

/// The three pipe threads of a call and what they reported.
struct Pipes {
    rx: mpsc::Receiver<Event>,
    threads: Vec<JoinHandle<()>>,
    out: Option<Stdout>,
    err: Option<Vec<u8>>,
}

impl Pipes {
    fn take(&mut self, e: Event) {
        match e {
            Event::Written => {}
            Event::Stdout(s) => self.out = Some(s),
            Event::Stderr(s) => self.err = Some(s),
        }
    }

    /// Wait up to `d` for one event.
    fn poll(&mut self, d: Duration) {
        if let Ok(e) = self.rx.recv_timeout(d) {
            self.take(e);
        }
        while let Ok(e) = self.rx.try_recv() {
            self.take(e);
        }
    }

    /// Wait up to `grace` for every thread to finish, join those that did,
    /// and leave the rest (blocked on a pipe some other process still holds)
    /// to end when that pipe closes.
    fn settle(&mut self, grace: Duration) {
        let end = Instant::now() + grace;
        loop {
            while let Ok(e) = self.rx.try_recv() {
                self.take(e);
            }
            let (done, running): (Vec<_>, Vec<_>) =
                self.threads.drain(..).partition(|t| t.is_finished());
            self.threads = running;
            for t in done {
                let _ = t.join();
            }
            let now = Instant::now();
            if self.threads.is_empty() || now >= end {
                break;
            }
            self.poll((end - now).min(Duration::from_millis(5)));
        }
        while let Ok(e) = self.rx.try_recv() {
            self.take(e);
        }
    }

    fn stderr_text(&self) -> String {
        self.err
            .as_deref()
            .map(|b| clean_text(&String::from_utf8_lossy(b)))
            .unwrap_or_default()
    }
}

/// Owns the running program: kills (its whole process group on Unix) and
/// reaps it unless it was already reaped.
struct ChildGuard {
    child: Child,
    reaped: bool,
}

impl ChildGuard {
    fn new(child: Child) -> ChildGuard {
        ChildGuard {
            child,
            reaped: false,
        }
    }

    fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child.stdin.take()
    }

    fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.stdout.take()
    }

    fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.child.stderr.take()
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let s = self.child.try_wait()?;
        if s.is_some() {
            self.reaped = true;
        }
        Ok(s)
    }

    /// Kill the program and every process left in its group, then reap the
    /// program.
    fn kill(&mut self) {
        kill_group(self.child.id());
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.reaped {
            self.kill();
        }
    }
}

/// Send SIGKILL to process group `pgid` (the program was started as its
/// leader). Best effort: the standard library has no `killpg`, so this runs
/// `kill(1)`; the program itself is also killed directly. While processes of
/// the group remain, the kernel keeps the group id from being reused.
#[cfg(unix)]
fn kill_group(pgid: u32) {
    let Ok(pgid) = i32::try_from(pgid) else {
        return;
    };
    if pgid <= 1 {
        return;
    }
    let target = format!("-{pgid}");
    for kill in ["/bin/kill", "/usr/bin/kill"] {
        if Path::new(kill).is_file() {
            let r = Command::new(kill)
                .args(["-KILL", "--", &target])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if r.is_ok() {
                return;
            }
        }
    }
}

#[cfg(not(unix))]
fn kill_group(_pgid: u32) {}

impl Plugin for ProcessPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn build(&self, req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
        let v = self.call(&serde_json::json!({"method": "build", "request": req}))?;
        let ops: Vec<Op> = serde_json::from_value(v)
            .map_err(|e| PluginError::Failed(format!("{}: operations: {e}", self.manifest.id)))?;
        crate::check_ops(req, &ops)?;
        Ok(ops)
    }

    fn analyze(&self, req: &AnalysisRequest) -> Result<serde_json::Value, PluginError> {
        self.call(&serde_json::json!({"method": "analyze", "request": req}))
    }

    fn validate_data(&self, data: &serde_json::Value) -> Result<(), PluginError> {
        self.call(&serde_json::json!({"method": "validate_data", "data": data}))
            .map(|_| ())
    }

    fn data_effects(&self, data: &serde_json::Value) -> crate::DataEffects {
        self.call(&serde_json::json!({"method": "data_effects", "data": data}))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn rule_symbols(
        &self,
        data: &serde_json::Value,
        body: Option<u32>,
    ) -> std::collections::BTreeMap<String, String> {
        self.call(&serde_json::json!({"method": "rule_symbols", "data": data, "body": body}))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }
}

/// Directories searched for installed plugins: `RCAD_PLUGIN_PATH`
/// (`:`-separated, `;` on Windows; relative entries are ignored) first, then
/// `<data dir>/r-cad/plugins` (`$XDG_DATA_HOME`, else `~/.local/share`;
/// `%APPDATA%` on Windows).
pub fn search_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("RCAD_PLUGIN_PATH") {
        v.extend(std::env::split_paths(&p).filter(|p| p.is_absolute()));
    }
    let data = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|h| h.join(".local/share"))
            })
    };
    if let Some(d) = data {
        v.push(d.join("r-cad").join("plugins"));
    }
    v
}

/// Load and register every plugin under `dirs` (each subdirectory with a
/// `plugin.json`). A built-in plugin's id is never replaced by an external
/// one, and when two directories hold the same id the first (in `dirs`
/// order, then by path) wins. Returns the registered ids and the
/// per-directory errors.
pub fn load_installed(
    dirs: &[PathBuf],
    builtin: &[&str],
) -> (Vec<String>, Vec<(PathBuf, PluginError)>) {
    let mut ok = Vec::new();
    let mut errs = Vec::new();
    let mut seen = BTreeSet::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        let mut subs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join("plugin.json").is_file())
            .collect();
        subs.sort();
        for s in subs {
            match ProcessPlugin::load(&s) {
                Ok(p) if builtin.contains(&p.manifest.id.as_str()) => errs.push((
                    s,
                    PluginError::Manifest(format!(
                        "'{}' is a built-in plugin id; not replaced",
                        p.manifest.id
                    )),
                )),
                Ok(p) if seen.contains(&p.manifest.id) => errs.push((
                    s,
                    PluginError::Manifest(format!(
                        "'{}' is already installed from an earlier directory; not replaced",
                        p.manifest.id
                    )),
                )),
                Ok(p) => {
                    let id = p.manifest.id.clone();
                    match crate::register(Arc::new(p)) {
                        Ok(()) => {
                            seen.insert(id.clone());
                            ok.push(id);
                        }
                        Err(e) => errs.push((s, e)),
                    }
                }
                Err(e) => errs.push((s, e)),
            }
        }
    }
    (ok, errs)
}
