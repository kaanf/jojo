//! M5: the one surface that reaches a remote. Two programs are started here, each in a
//! closed argument vocabulary: `/usr/bin/git` (`push` of one commit to one branch, never
//! forced; `ls-remote`; `config --get`) and the `gh` that discovery found (`pr create
//! --draft`, `pr list`, `auth status`). Both get a cleared environment with the user's
//! HOME, so that git's credential helper and gh's own sign-in are used as they are; no
//! token is read, passed or stored. Nothing here decides or records anything: the
//! caller records what was observed, and nothing is ever sent twice.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
const POLL: Duration = Duration::from_millis(20);

/// How long a push or a pull request may take before its outcome is unknown.
pub const ACTION_BUDGET: Duration = Duration::from_secs(120);
/// How long a read of the remote (`ls-remote`, `pr list`, `auth status`) may take.
pub const READ_BUDGET: Duration = Duration::from_secs(30);

/// The environment both programs get: the user's HOME and a fixed PATH.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEnv {
    pub home: PathBuf,
    /// `/usr/bin:/bin`, and the directory of `gh` when it was found, so that a credential
    /// helper configured as `gh auth git-credential` resolves.
    pub path: String,
}
impl RemoteEnv {
    pub fn new(home: &Path, gh: Option<&Path>) -> Self {
        let mut path = "/usr/bin:/bin".to_owned();
        if let Some(dir) = gh.and_then(Path::parent).and_then(Path::to_str) {
            path = format!("{dir}:{path}");
        }
        Self {
            home: home.to_owned(),
            path,
        }
    }
}

/// What one push or pull request did, as far as its program told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Committed {
        detail: String,
        number: Option<u64>,
        url: Option<String>,
    },
    /// Certainly not performed: the remote refused, or the program failed before it
    /// reached the remote.
    Rejected(String),
    /// Maybe performed: the program did not finish, or failed in a way that does not say.
    Unknown(String),
}

/// A finished or abandoned run. `status` is `None` when it ran out of time and was
/// killed.
struct Ran {
    status: Option<ExitStatus>,
    stdout: String,
    stderr: String,
}
impl Ran {
    fn failed(why: String) -> Self {
        Self {
            status: None,
            stdout: String::new(),
            stderr: why,
        }
    }
    fn ok(&self) -> bool {
        self.status.is_some_and(|s| s.success())
    }
    fn first_error(&self) -> String {
        let line = self
            .stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("no message");
        line.chars().take(512).collect()
    }
}

fn base_env(site: &RemoteEnv) -> [(&'static str, String); 3] {
    [
        ("HOME", site.home.display().to_string()),
        ("PATH", site.path.clone()),
        ("LC_ALL", "C".to_owned()),
    ]
}

fn git(site: &RemoteEnv, cwd: &Path, args: &[&str], budget: Duration) -> Ran {
    if !matches!(args.first().copied(), Some("push" | "ls-remote" | "config"))
        || (args.first() == Some(&"config") && args.get(1) != Some(&"--get"))
    {
        return Ran::failed("unsupported remote Git command".into());
    }
    let mut launch = Command::new("/usr/bin/git");
    launch
        .env_clear()
        .envs(base_env(site))
        .envs([("GIT_TERMINAL_PROMPT", "0"), ("GIT_OPTIONAL_LOCKS", "0")])
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .arg("-C")
        .arg(cwd)
        .args(args);
    run(launch, None, budget)
}

fn gh(
    site: &RemoteEnv,
    gh: &Path,
    cwd: &Path,
    args: &[&str],
    input: Option<&str>,
    budget: Duration,
) -> Ran {
    if !matches!(args.first().copied(), Some("pr" | "auth"))
        || !matches!(args.get(1).copied(), Some("create" | "list" | "status"))
        || (args.get(1) == Some(&"create") && args.get(2) != Some(&"--draft"))
    {
        return Ran::failed("unsupported gh command".into());
    }
    let mut launch = Command::new(gh);
    launch
        .env_clear()
        .envs(base_env(site))
        .envs([
            ("GH_PROMPT_DISABLED", "1"),
            ("GH_NO_UPDATE_NOTIFIER", "1"),
            ("GH_SPINNER_DISABLED", "1"),
            ("NO_COLOR", "1"),
        ])
        .current_dir(cwd)
        .args(args);
    run(launch, input, budget)
}

/// Runs a launch within `budget`, with standard output and error captured.
fn run(mut launch: Command, input: Option<&str>, budget: Duration) -> Ran {
    let spawned = launch
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return Ran::failed(format!("start: {e}")),
    };
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        // A program that stops reading early is judged by its exit status.
        let _ = stdin.write_all(text.as_bytes());
    }
    let collect = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::Builder::new()
            .name("jojo-remote-output".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                if let Some(pipe) = pipe {
                    let _ = pipe.take(MAX_OUTPUT_BYTES).read_to_end(&mut bytes);
                }
                String::from_utf8_lossy(&bytes).into_owned()
            })
            .ok()
    };
    let out = collect(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = collect(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let polls = budget.as_millis() / POLL.as_millis();
    let mut status = None;
    for _ in 0..polls {
        if let Ok(Some(exit)) = child.try_wait() {
            status = Some(exit);
            break;
        }
        std::thread::sleep(POLL);
    }
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let join = |h: Option<std::thread::JoinHandle<String>>| {
        h.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ran {
        status,
        stdout: join(out),
        stderr: join(err),
    }
}

/// The URL of the repository's remote `name`, as its configuration says; nothing is read
/// from the network.
pub fn remote_url(site: &RemoteEnv, repository: &Path, name: &str) -> Result<String, String> {
    let key = format!("remote.{name}.url");
    let ran = git(site, repository, &["config", "--get", &key], READ_BUDGET);
    let url = ran.stdout.trim().to_owned();
    if !ran.ok() || url.is_empty() {
        return Err(format!("the repository has no remote named {name}"));
    }
    Ok(url)
}

/// The branch the remote's HEAD names, read from the remote.
pub fn default_branch(site: &RemoteEnv, repository: &Path, url: &str) -> Result<String, String> {
    let ran = git(
        site,
        repository,
        &["ls-remote", "--symref", url, "HEAD"],
        READ_BUDGET,
    );
    if !ran.ok() {
        return Err(format!(
            "the remote could not be read: {}",
            ran.first_error()
        ));
    }
    ran.stdout
        .lines()
        .find_map(|l| l.strip_prefix("ref: refs/heads/"))
        .and_then(|rest| rest.split('\t').next())
        .filter(|b| !b.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "the remote names no default branch".into())
}

/// The commit `branch` names on the remote, or `None` when the remote has no such branch.
pub fn remote_branch(
    site: &RemoteEnv,
    repository: &Path,
    url: &str,
    branch: &str,
) -> Result<Option<String>, String> {
    let reference = format!("refs/heads/{branch}");
    let ran = git(
        site,
        repository,
        &["ls-remote", url, &reference],
        READ_BUDGET,
    );
    if !ran.ok() {
        return Err(format!(
            "the remote could not be read: {}",
            ran.first_error()
        ));
    }
    Ok(ran
        .stdout
        .lines()
        .find(|l| l.ends_with(&format!("\t{reference}")))
        .and_then(|l| l.split('\t').next())
        .map(str::to_owned))
}

/// The credential helpers git would use, as its configuration says.
pub fn credential_helpers(site: &RemoteEnv, repository: &Path) -> Vec<String> {
    let ran = git(
        site,
        repository,
        &["config", "--get", "credential.helper"],
        READ_BUDGET,
    );
    ran.stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Failures that happen before git or gh reaches the remote's data: nothing changed.
fn before_transfer(stderr: &str) -> bool {
    [
        "Authentication failed",
        "could not read Username",
        "could not read Password",
        "Invalid username or password",
        "Repository not found",
        "Could not resolve host",
        "unable to access",
        "does not appear to be a git repository",
        "Permission to",
        "gh auth login",
        "not logged in",
        "HTTP 401",
        "HTTP 403",
        "Could not resolve to a Repository",
        "must first push",
    ]
    .iter()
    .any(|m| stderr.contains(m))
}

/// Pushes one commit to one branch of `url`, once. Never forced: a remote branch at
/// another commit refuses it.
pub fn push(site: &RemoteEnv, repository: &Path, url: &str, commit: &str, branch: &str) -> Outcome {
    let refspec = format!("{commit}:refs/heads/{branch}");
    let ran = git(
        site,
        repository,
        &["push", "--porcelain", "--no-verify", url, &refspec],
        ACTION_BUDGET,
    );
    let Some(status) = ran.status else {
        return Outcome::Unknown(format!(
            "git push did not finish ({}); the remote may or may not have the branch",
            ran.first_error()
        ));
    };
    let reference = format!("refs/heads/{branch}");
    let line = ran
        .stdout
        .lines()
        .find(|l| l.contains('\t') && l.contains(&reference));
    let summary = |l: &str| l.split('\t').nth(2).unwrap_or("").trim().to_owned();
    match line.and_then(|l| l.chars().next().map(|flag| (flag, l))) {
        Some(('!', l)) => Outcome::Rejected(format!("the remote refused the push: {}", summary(l))),
        Some((' ' | '*' | '=', l)) if status.success() => Outcome::Committed {
            detail: match summary(l).as_str() {
                "" => "pushed".to_owned(),
                s => format!("pushed ({s})"),
            },
            number: None,
            url: None,
        },
        _ if !status.success() && before_transfer(&ran.stderr) => {
            Outcome::Rejected(format!("git push failed: {}", ran.first_error()))
        }
        _ => Outcome::Unknown(format!(
            "git push exited with {status} without saying what the remote did: {}",
            ran.first_error()
        )),
    }
}

/// The `--repo` argument for gh: `host/owner/name` of an HTTPS URL, else the URL itself.
fn repo_argument(url: &str) -> String {
    match url.strip_prefix("https://") {
        Some(rest) => rest
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_owned(),
        None => url.to_owned(),
    }
}

fn pull_number(url: &str) -> Option<u64> {
    url.rsplit_once("/pull/")
        .and_then(|(_, n)| n.trim_end_matches('/').parse().ok())
}

/// Opens one draft pull request from `branch` into `base`, once.
#[allow(clippy::too_many_arguments)]
pub fn create_draft(
    site: &RemoteEnv,
    gh_path: &Path,
    repository: &Path,
    url: &str,
    base: &str,
    branch: &str,
    title: &str,
    body: &str,
) -> Outcome {
    let repo = repo_argument(url);
    let ran = gh(
        site,
        gh_path,
        repository,
        &[
            "pr",
            "create",
            "--draft",
            "--repo",
            &repo,
            "--base",
            base,
            "--head",
            branch,
            "--title",
            title,
            "--body-file",
            "-",
        ],
        Some(body),
        ACTION_BUDGET,
    );
    let Some(status) = ran.status else {
        return Outcome::Unknown(format!(
            "gh pr create did not finish ({}); a pull request may or may not exist",
            ran.first_error()
        ));
    };
    if status.success() {
        let link = ran
            .stdout
            .lines()
            .map(str::trim)
            .find(|l| l.contains("/pull/"))
            .map(str::to_owned);
        return match link.as_deref().and_then(pull_number) {
            Some(number) => Outcome::Committed {
                detail: format!("draft pull request #{number} opened"),
                number: Some(number),
                url: link,
            },
            None => Outcome::Unknown("gh reported success without a pull request link".into()),
        };
    }
    if before_transfer(&ran.stderr) {
        Outcome::Rejected(format!("gh pr create failed: {}", ran.first_error()))
    } else {
        Outcome::Unknown(format!(
            "gh pr create exited with {status}: {}",
            ran.first_error()
        ))
    }
}

/// A pull request whose head is `branch`: its number, link and whether it is a draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoundPullRequest {
    pub number: u64,
    pub url: String,
    pub draft: bool,
}

/// The newest pull request of any state whose head is `branch`.
pub fn find_pull_request(
    site: &RemoteEnv,
    gh_path: &Path,
    repository: &Path,
    url: &str,
    branch: &str,
) -> Result<Option<FoundPullRequest>, String> {
    let repo = repo_argument(url);
    let ran = gh(
        site,
        gh_path,
        repository,
        &[
            "pr",
            "list",
            "--repo",
            &repo,
            "--head",
            branch,
            "--state",
            "all",
            "--json",
            "number,url,isDraft",
            "--limit",
            "5",
        ],
        None,
        READ_BUDGET,
    );
    if !ran.ok() {
        return Err(format!("gh pr list failed: {}", ran.first_error()));
    }
    let parsed = jojo_domain::json::parse(ran.stdout.trim().as_bytes())
        .map_err(|_| "gh pr list printed no JSON".to_owned())?;
    let jojo_domain::json::Json::List(items) = parsed else {
        return Err("gh pr list printed no list".into());
    };
    let Some(first) = items.first() else {
        return Ok(None);
    };
    let field = |k: &str| jojo_domain::ipc::get(first, k).ok();
    let number = field("number")
        .and_then(|n| jojo_domain::ipc::uint(n).ok())
        .ok_or("gh pr list gave no number")?;
    let link = field("url")
        .and_then(|u| jojo_domain::ipc::string(u).ok())
        .unwrap_or("")
        .to_owned();
    let draft = field("isDraft") == Some(&jojo_domain::json::Json::Bool(true));
    Ok(Some(FoundPullRequest {
        number,
        url: link,
        draft,
    }))
}

/// Whether gh is signed in, by its own `auth status`; the first line it printed.
pub fn gh_signed_in(site: &RemoteEnv, gh_path: &Path, repository: &Path) -> Result<String, String> {
    let ran = gh(
        site,
        gh_path,
        repository,
        &["auth", "status"],
        None,
        READ_BUDGET,
    );
    let text = format!("{}\n{}", ran.stdout, ran.stderr);
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.contains("Logged in") || l.contains("not logged"))
        .unwrap_or_else(|| {
            text.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("")
        })
        .trim_start_matches(['✓', 'X', '!', ' '])
        .chars()
        .take(256)
        .collect::<String>();
    if ran.ok() {
        Ok(line)
    } else if ran.status.is_none() {
        Err(format!(
            "gh auth status did not finish: {}",
            ran.first_error()
        ))
    } else {
        Err(if line.is_empty() {
            "gh is not signed in; run gh auth login".into()
        } else {
            line
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m5_gh_gets_host_owner_and_name_and_a_pull_request_link_its_number() {
        assert_eq!(
            repo_argument("https://github.com/kaanf/jojo.git"),
            "github.com/kaanf/jojo"
        );
        assert_eq!(
            repo_argument("https://github.com/kaanf/jojo/"),
            "github.com/kaanf/jojo"
        );
        assert_eq!(repo_argument("file:///tmp/r.git"), "file:///tmp/r.git");
        assert_eq!(
            pull_number("https://github.com/kaanf/jojo/pull/12"),
            Some(12)
        );
        assert_eq!(pull_number("https://github.com/kaanf/jojo"), None);
        assert!(before_transfer(
            "fatal: Authentication failed for 'https://github.com/x'"
        ));
        assert!(!before_transfer("error: RPC failed; curl 56 Recv failure"));
        assert_eq!(
            RemoteEnv::new(Path::new("/Users/u"), Some(Path::new("/opt/x/bin/gh"))).path,
            "/opt/x/bin:/usr/bin:/bin"
        );
    }
}
