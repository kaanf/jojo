//! D3: finding a provider CLI the way the user's login shell would, and learning its
//! version and digest.
//!
//! The login shell is asked one thing, its `PATH`, by a fixed script; the lookup itself
//! happens here. The entry found is resolved to the native binary it stands for: a
//! symbolic link to its target, and the npm launcher of Codex (`bin/codex.js`, a Node
//! script that cannot run in a cleared environment) to the platform binary its package
//! ships, by the layout that launcher itself uses. Anything else that is not a native
//! executable is refused.
//!
//! The version is the first line `<binary> --version` prints, with the cleared
//! environment of the provider's launch envelope; no credential is involved. Version and
//! digest are learned on a background thread, once per file identity
//! (`executables::file_key`), so a caller on the control thread only stats files and is
//! told `Checking` meanwhile. Two programs are ever started here: the validated login
//! shell with the fixed script, and a found binary with `--version`.
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use jojo_domain::claude::CLAUDE_VERSION;
use jojo_domain::codex::CODEX_VERSION;
use jojo_domain::provider::{ProviderKind, plain_version};

use crate::executables::{FileKey, file_key, inspect_executable};

/// The login shells that are asked for `PATH`. They take `-ilc` and the POSIX script.
const SHELLS: [&str; 3] = ["zsh", "bash", "sh"];
/// The PATH sits between two marks, so that what a profile prints around it is ignored.
const MARK: &str = "__JOJO_PATH__";
const SCRIPT: &str = "printf '\\n__JOJO_PATH__%s__JOJO_PATH__\\n' \"$PATH\"";
/// How long the login shell and a `--version` probe may take, in polls of `POLL`.
const POLLS: u32 = 500;
const POLL: Duration = Duration::from_millis(20);
const MAX_OUTPUT_BYTES: u64 = 64 * 1024;

/// The name a provider's CLI has on `PATH`.
pub fn command_name(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ClaudeCode => "claude",
        ProviderKind::Codex => "codex",
    }
}

/// The release a provider's stream profiles were written against.
pub fn profiled_version(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ClaudeCode => CLAUDE_VERSION,
        ProviderKind::Codex => CODEX_VERSION,
    }
}

/// An executable as discovery found it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoundExecutable {
    /// The `PATH` entry, when it was looked up there (`None` for a launcher option).
    pub entry: Option<PathBuf>,
    /// The native binary itself, fully resolved. This is what runs.
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
    /// Whether the version is the one the stream profiles were written against.
    pub profiled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lookup {
    /// Being looked up, probed or hashed on a background thread; ask again shortly.
    Checking,
    Found(FoundExecutable),
    /// Cannot be run; the text says why.
    Refused(String),
}

/// Where `PATH` comes from.
#[derive(Clone, Debug)]
enum Source {
    Shell {
        shell: PathBuf,
        home: PathBuf,
    },
    /// Tests: the value a login shell would have printed.
    Given(String),
}

#[derive(Debug, Default)]
enum SearchPath {
    #[default]
    Never,
    Capturing,
    Known {
        value: Result<String, String>,
        generation: u64,
    },
}

type Probe = Result<(String, String), String>;

#[derive(Debug, Default)]
struct Inner {
    search: SearchPath,
    generation: u64,
    /// The generation of `PATH` captured again after a name was missing from it.
    retried: HashMap<&'static str, u64>,
    probed: HashMap<PathBuf, (FileKey, Probe)>,
    probing: HashSet<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Discovery {
    source: Source,
    inner: Arc<Mutex<Inner>>,
}

fn locked(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Discovery {
    /// Discovery through `shell`, which must be an absolute zsh, bash or sh listed in
    /// `/etc/shells`. `home` is the only variable the shell gets.
    pub fn login_shell(shell: &Path, home: &Path) -> Result<Self, String> {
        let name = shell.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !shell.is_absolute() || !SHELLS.contains(&name) {
            return Err(format!(
                "{}: not an absolute zsh, bash or sh",
                shell.display()
            ));
        }
        let listed =
            std::fs::read_to_string("/etc/shells").map_err(|e| format!("/etc/shells: {e}"))?;
        if !listed.lines().any(|l| Path::new(l.trim()) == shell) {
            return Err(format!("{}: not listed in /etc/shells", shell.display()));
        }
        if !home.is_absolute() {
            return Err("home is not absolute".into());
        }
        Ok(Self::new(Source::Shell {
            shell: shell.to_owned(),
            home: home.to_owned(),
        }))
    }

    /// Discovery over a given `PATH` value instead of a login shell's.
    pub fn with_search_path(value: impl Into<String>) -> Self {
        Self::new(Source::Given(value.into()))
    }

    fn new(source: Source) -> Self {
        Self {
            source,
            inner: Arc::default(),
        }
    }

    /// Starts capturing the login shell's `PATH` if that never happened.
    pub fn begin(&self) {
        let mut inner = locked(&self.inner);
        if matches!(inner.search, SearchPath::Never) {
            self.capture(&mut inner);
        }
    }

    /// Returns at once. `executable` is the launcher's path option, which replaces the
    /// lookup; `environment` is what the `--version` probe gets.
    pub fn find(
        &self,
        provider: ProviderKind,
        executable: Option<&Path>,
        environment: &[(String, String)],
    ) -> Lookup {
        let (entry, path) = match executable {
            Some(path) => (None, path.to_owned()),
            None => match self.search(command_name(provider)) {
                None => return Lookup::Checking,
                Some(Err(why)) => return Lookup::Refused(why),
                Some(Ok(entry)) => match native(&entry, provider) {
                    Ok(path) => (Some(entry), path),
                    Err(why) => return Lookup::Refused(format!("{}: {why}", entry.display())),
                },
            },
        };
        let key = match file_key(&path) {
            Ok(key) => key,
            Err(why) => return Lookup::Refused(format!("{}: {why}", path.display())),
        };
        let mut inner = locked(&self.inner);
        if let Some((known, probe)) = inner.probed.get(&path)
            && *known == key
        {
            return match probe {
                Ok((version, sha256)) => Lookup::Found(FoundExecutable {
                    entry,
                    path,
                    profiled: version == profiled_version(provider),
                    version: version.clone(),
                    sha256: sha256.clone(),
                }),
                Err(why) => Lookup::Refused(format!("{}: {why}", path.display())),
            };
        }
        if !inner.probing.insert(path.clone()) {
            return Lookup::Checking;
        }
        drop(inner);
        let shared = self.inner.clone();
        let environment = environment.to_vec();
        let owned = path.clone();
        let spawned = std::thread::Builder::new()
            .name("jojo-executable-probe".into())
            .spawn(move || {
                let probe = version_of(&owned, &environment)
                    .and_then(|version| inspect_executable(&owned).map(|digest| (version, digest)));
                let mut inner = locked(&shared);
                // A file that changed while it was checked is checked again on next ask.
                if file_key(&owned).is_ok_and(|now| now == key) {
                    inner.probed.insert(owned.clone(), (key, probe));
                }
                inner.probing.remove(&owned);
            });
        if spawned.is_err() {
            locked(&self.inner).probing.remove(&path);
            return Lookup::Refused("no thread for the executable check".into());
        }
        Lookup::Checking
    }

    /// The first entry of `name` on `PATH`; `None` while `PATH` is being captured. A name
    /// that is missing has `PATH` captured once more before it is reported missing.
    fn search(&self, name: &'static str) -> Option<Result<PathBuf, String>> {
        let mut inner = locked(&self.inner);
        // Never captured, then missing once: at most two captures before an answer.
        for _ in 0..3 {
            let (found, generation, failure) = match &inner.search {
                SearchPath::Never => {
                    self.capture(&mut inner);
                    continue;
                }
                SearchPath::Capturing => return None,
                SearchPath::Known { value, generation } => (
                    value.as_ref().ok().and_then(|v| lookup(name, v)),
                    *generation,
                    value.as_ref().err().cloned(),
                ),
            };
            if let Some(entry) = found {
                return Some(Ok(entry));
            }
            if inner.retried.get(name) == Some(&generation) {
                return Some(Err(failure.map_or_else(
                    || format!("`{name}` is not on the login shell's PATH"),
                    |why| format!("the login shell gave no PATH: {why}"),
                )));
            }
            inner.retried.insert(name, generation + 1);
            self.capture(&mut inner);
        }
        None
    }

    /// Captures `PATH` again: at once for a given value, on a background thread for a
    /// login shell.
    fn capture(&self, inner: &mut Inner) {
        let generation = inner.generation + 1;
        inner.generation = generation;
        match &self.source {
            Source::Given(value) => {
                inner.search = SearchPath::Known {
                    value: Ok(value.clone()),
                    generation,
                };
            }
            Source::Shell { shell, home } => {
                inner.search = SearchPath::Capturing;
                let (shell, home, shared) = (shell.clone(), home.clone(), self.inner.clone());
                let spawned = std::thread::Builder::new()
                    .name("jojo-login-shell".into())
                    .spawn(move || {
                        let value = shell_path(&shell, &home);
                        locked(&shared).search = SearchPath::Known { value, generation };
                    });
                if spawned.is_err() {
                    inner.search = SearchPath::Known {
                        value: Err("no thread for the login shell".into()),
                        generation,
                    };
                }
            }
        }
    }
}

/// The first executable file named `name` in an absolute directory of `search`.
fn lookup(name: &str, search: &str) -> Option<PathBuf> {
    search
        .split(':')
        .filter(|directory| directory.starts_with('/'))
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| {
            std::fs::metadata(candidate)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

fn native_header(path: &Path) -> bool {
    let mut head = [0u8; 4];
    std::fs::File::open(path).is_ok_and(|mut f| f.read_exact(&mut head).is_ok())
        && matches!(
            head,
            [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf]
        )
}

/// The native binary a `PATH` entry stands for.
fn native(entry: &Path, provider: ProviderKind) -> Result<PathBuf, String> {
    let real = std::fs::canonicalize(entry).map_err(|e| format!("resolve: {e}"))?;
    if native_header(&real) {
        return Ok(real);
    }
    if provider == ProviderKind::Codex
        && let Some(binary) = codex_binary(&real)
    {
        return Ok(binary);
    }
    Err("a script launcher, not a native executable".into())
}

/// The platform binary the npm package of Codex ships beside its `bin/codex.js`
/// launcher: in the platform package it depends on, or in its own `vendor` directory.
fn codex_binary(launcher: &Path) -> Option<PathBuf> {
    let bin = launcher.parent()?;
    let package = bin.parent()?;
    if launcher.file_name()? != "codex.js"
        || bin.file_name()? != "bin"
        || !package.ends_with("@openai/codex")
    {
        return None;
    }
    let (platform, triple) = if cfg!(target_arch = "aarch64") {
        ("codex-darwin-arm64", "aarch64-apple-darwin")
    } else {
        ("codex-darwin-x64", "x86_64-apple-darwin")
    };
    let vendored = |root: PathBuf| root.join("vendor").join(triple).join("bin/codex");
    [
        vendored(package.join("node_modules/@openai").join(platform)),
        vendored(package.to_owned()),
    ]
    .into_iter()
    .find_map(|candidate| {
        std::fs::canonicalize(candidate)
            .ok()
            .filter(|real| native_header(real))
    })
}

/// The login shell's `PATH`, from the fixed script with only `HOME` set.
fn shell_path(shell: &Path, home: &Path) -> Result<String, String> {
    let mut launch = Command::new(shell);
    launch
        .args(["-ilc", SCRIPT])
        .env_clear()
        .envs([("HOME", home)]);
    let output = run(launch)?;
    let text = String::from_utf8_lossy(&output);
    let mut parts = text.split(MARK);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(value), Some(_)) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        _ => Err("no PATH between the marks".into()),
    }
}

/// The first line `executable --version` prints with exactly `environment`.
fn version_of(executable: &Path, environment: &[(String, String)]) -> Result<String, String> {
    let mut launch = Command::new(executable);
    launch
        .arg("--version")
        .env_clear()
        .envs(environment.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let output = run(launch)?;
    let text = String::from_utf8_lossy(&output);
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if !plain_version(line) {
        return Err("--version printed no usable version".into());
    }
    Ok(line.to_owned())
}

/// Runs a launch with no input and standard output captured, within the poll budget.
fn run(mut launch: Command) -> Result<Vec<u8>, String> {
    let mut child = launch
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("start: {e}"))?;
    let stdout = child.stdout.take().ok_or("no standard output")?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::Builder::new()
        .name("jojo-discovery-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let read = stdout.take(MAX_OUTPUT_BYTES + 1).read_to_end(&mut bytes);
            let _ = tx.send(read.map(|_| bytes));
        });
    if reader.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("no thread for the output".into());
    }
    let mut status = None;
    for _ in 0..POLLS {
        if let Ok(Some(exit)) = child.try_wait() {
            status = Some(exit);
            break;
        }
        std::thread::sleep(POLL);
    }
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("timed out".into());
    };
    let bytes = rx
        .recv_timeout(POLL * 50)
        .map_err(|_| "output not closed".to_owned())?
        .map_err(|e| format!("read: {e}"))?;
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err("too much output".into());
    }
    if !status.success() {
        // A profile that fails after printing PATH still counts; the marks decide.
        if bytes.is_empty() {
            return Err(format!("exited with {status}"));
        }
    }
    Ok(bytes)
}
