//! T02 with the real helper executable: helper B is denied while helper A owns the
//! database; after A is killed, a replacement starts a new epoch with admission closed.
//! The only program launched is the `jojo-helper` binary built from this workspace, with
//! an empty environment and a temporary data root.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("jojo-helper-t02-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn data(&self) -> PathBuf {
        self.0.join("data")
    }

    fn worktrees(&self) -> PathBuf {
        self.0.join("worktrees")
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn helper(root: &TempRoot) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jojo-helper"));
    command
        .arg("--engine-probe")
        .arg("--data-root")
        .arg(root.data())
        .arg("--worktree-root")
        .arg(root.worktrees())
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Starts a helper and waits for its ready line (60 s is a failure bound only).
fn start_owner(root: &TempRoot) -> (Child, String) {
    let mut child = helper(root).spawn().expect("spawn helper");
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = stderr.read_line(&mut line);
        let _ = sender.send((line, stderr));
    });
    match receiver.recv_timeout(Duration::from_secs(60)) {
        Ok((line, stderr)) if line.contains("engine ready") => {
            child.stderr = Some(stderr.into_inner());
            (child, line.trim().to_string())
        }
        other => {
            let _ = child.kill();
            panic!(
                "helper did not become ready: {:?}",
                other.map(|(line, _)| line)
            );
        }
    }
}

/// Closes the helper's stdin (the GUI going away) and returns (exit code, stdout, stderr).
fn close_and_wait(mut child: Child) -> (Option<i32>, String, String) {
    drop(child.stdin.take());
    let mut stdout = String::new();
    let mut stderr = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    (child.wait().unwrap().code(), stdout, stderr)
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn second_helper_is_denied_and_a_replacement_after_sigkill_starts_a_new_closed_epoch() {
    let root = TempRoot::new("ownership");

    let (mut first, ready) = start_owner(&root);
    assert!(
        ready.ends_with(
            "engine ready epoch=1 schema=20 admission=closed previous=none quarantined_claims=0"
        ),
        "{ready}"
    );
    assert_eq!(mode(&root.data()), 0o700, "the data root is private");
    assert!(
        !root.worktrees().exists(),
        "the probe does not create the worktree root"
    );

    // Helper B: refused, without waiting and without touching the database.
    let denied = helper(&root).output().expect("run second helper");
    assert_eq!(denied.status.code(), Some(3));
    let message = String::from_utf8_lossy(&denied.stderr);
    assert!(
        message.contains(&format!(
            "owned by another helper (pid={} epoch=1)",
            first.id()
        )),
        "{message}"
    );
    assert!(
        denied.stdout.is_empty(),
        "stdout is reserved for the future protocol"
    );

    // Helper A dies abruptly.
    // Hold stdin open until the helper is reaped: `wait` would otherwise close it right
    // after the signal is posted, and EOF could win the race against SIGKILL.
    let stdin = first.stdin.take();
    first.kill().unwrap();
    assert_eq!(first.wait().unwrap().signal(), Some(9));
    drop(stdin);

    // Helper C replaces it: new epoch, admission closed, predecessor recorded as unclean.
    let (third, ready) = start_owner(&root);
    assert!(
        ready.ends_with(
            "engine ready epoch=2 schema=20 admission=closed previous=unclean quarantined_claims=0"
        ),
        "{ready}"
    );
    let (code, stdout, stderr) = close_and_wait(third);
    assert_eq!(code, Some(0));
    assert!(stdout.is_empty());
    assert!(stderr.contains("clean shutdown recorded"), "{stderr}");

    // After a clean shutdown the next helper still starts closed, in yet another epoch.
    let (fourth, ready) = start_owner(&root);
    assert!(
        ready.ends_with(
            "engine ready epoch=3 schema=20 admission=closed previous=clean quarantined_claims=0"
        ),
        "{ready}"
    );
    assert_eq!(close_and_wait(fourth).0, Some(0));
}

#[test]
fn a_data_root_readable_by_others_is_refused() {
    let root = TempRoot::new("not-private");
    std::fs::create_dir_all(root.data()).unwrap();
    std::fs::set_permissions(root.data(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let refused = helper(&root).output().unwrap();
    assert_eq!(refused.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("DataRootNotPrivate"));
    assert_eq!(
        std::fs::read_dir(root.data()).unwrap().count(),
        0,
        "nothing was created"
    );
    assert_eq!(
        mode(&root.data()),
        0o755,
        "permissions are not silently changed"
    );
}
