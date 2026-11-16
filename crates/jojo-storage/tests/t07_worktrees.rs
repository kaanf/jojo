//! T07 uses disposable local Git repositories, no providers, network or sleeps.
mod common;
use common::{TempDir, request, start};
use jojo_domain::dependency::*;
use jojo_domain::identity::{ArtifactId, FeatureId, OpaqueId, Revision};
use jojo_domain::provider::ProviderKind;
use jojo_domain::scheduler::*;
use jojo_storage::*;
use jojo_test_support::{
    COMPLETE_PRD_JSON,
    dependency::{node, plan},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
};
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .env_clear()
        .envs([
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
            ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
        ])
        .args([
            "-c",
            "user.name=T07",
            "-c",
            "user.email=t07@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
        ])
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}
fn repo(path: &Path) -> (LocalRepository, String) {
    fs::create_dir(path).unwrap();
    git(path, &["init", "--template=", "--initial-branch=main"]);
    fs::write(path.join("file"), "base\n").unwrap();
    fs::write(path.join("exec"), "echo original\n").unwrap();
    git(path, &["add", "--all"]);
    git(path, &["commit", "-m", "fixture"]);
    (
        LocalRepository::inspect(path).unwrap(),
        git(path, &["rev-parse", "HEAD"]),
    )
}
// Keep unrelated Git subprocess lifetimes out of restart tests. On Unix a concurrent
// fork can transiently inherit another fixture's advisory-lock descriptor before exec.
// Actual contention/overlap tests still use barriers and multiple attempts inside F.
static FIXTURE_SERIAL: (std::sync::Mutex<bool>, std::sync::Condvar) =
    (std::sync::Mutex::new(false), std::sync::Condvar::new());
struct FixtureSerial;
impl FixtureSerial {
    fn acquire() -> Self {
        let (lock, ready) = &FIXTURE_SERIAL;
        let mut held = lock.lock().unwrap();
        while *held {
            held = ready.wait(held).unwrap();
        }
        *held = true;
        Self
    }
}
impl Drop for FixtureSerial {
    fn drop(&mut self) {
        let (lock, ready) = &FIXTURE_SERIAL;
        *lock.lock().unwrap() = false;
        ready.notify_one();
    }
}
struct F {
    _serial: FixtureSerial,
    dir: TempDir,
    e: Engine,
    repo: LocalRepository,
    base: String,
    root: WorktreeRoot,
    next: AtomicU64,
}
impl F {
    fn new() -> Self {
        let serial = FixtureSerial::acquire();
        let dir = TempDir::new("t07");
        let (repo, base) = repo(&dir.path().join("repository with spaces ; literal"));
        let rp = dir.path().join("owned");
        fs::create_dir(&rp).unwrap();
        fs::set_permissions(&rp, fs::Permissions::from_mode(0o700)).unwrap();
        let root = WorktreeRoot::inspect(&rp).unwrap();
        let e = start(&dir.db());
        Self {
            _serial: serial,
            dir,
            e,
            repo,
            base,
            root,
            next: AtomicU64::new(1),
        }
    }
    fn cmd<FN>(&self, f: FN) -> Result<Receipt, CommandError>
    where
        FN: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        self.e.execute(
            request(
                &self.e,
                self.next.fetch_add(1, Ordering::SeqCst) as u128,
                "T07",
                "fixture",
            ),
            f,
        )
    }
    fn begin_repo(
        &self,
        a: u128,
        r: LocalRepository,
        p: Option<u128>,
    ) -> Result<String, CommandError> {
        let root = self.root.clone();
        let base = self.base.clone();
        self.cmd(move |tx| tx.request_worktree(OpaqueId(a), &r, &root, &base, p.map(OpaqueId)))
            .map(|v| v.response)
    }
    fn begin(&self, a: u128) -> String {
        self.begin_repo(a, self.repo.clone(), None).unwrap()
    }
    fn record(&self, a: u128) -> WorktreeRecord {
        self.e
            .reader()
            .unwrap()
            .read(|v| v.worktree(OpaqueId(a)))
            .unwrap()
            .unwrap()
    }
    fn finish(&self, a: u128, c: Option<&CandidateSnapshot>) {
        let proof = materialize_worktree(&self.e, OpaqueId(a), c).unwrap();
        self.cmd(move |tx| {
            tx.seal_worktree(proof)?;
            Ok("sealed".into())
        })
        .unwrap();
    }
    fn create(&self, a: u128) -> PathBuf {
        assert_eq!(self.begin(a), "CREATING");
        self.finish(a, None);
        self.record(a).path
    }
    fn candidate(&self, a: u128) -> CandidateSnapshot {
        let c = collect_candidate(&self.e, OpaqueId(a)).unwrap();
        let copy = c.clone();
        self.cmd(move |tx| {
            tx.record_candidate(&copy)?;
            Ok("candidate".into())
        })
        .unwrap();
        c
    }
    fn approve(&self, a: ArtifactId) {
        let b = self
            .e
            .reader()
            .unwrap()
            .read(|v| v.review_bundle(a))
            .unwrap()
            .unwrap();
        self.e
            .human_decide(
                CommandId(OpaqueId(self.next.fetch_add(1, Ordering::SeqCst) as u128)),
                HumanOrigin::FakeHumanFixture {
                    actor: "T07 human".into(),
                },
                b,
                DecisionAction::Approve,
            )
            .unwrap();
    }
    fn queued(&self, repos: &[LocalRepository], cap: u32) -> u64 {
        let (prd, adr, pid) = (
            ArtifactId(OpaqueId(901)),
            ArtifactId(OpaqueId(902)),
            ArtifactId(OpaqueId(903)),
        );
        let p = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
        self.cmd(move |tx| {
            tx.create_artifact(
                prd,
                FeatureId(OpaqueId(1)),
                &p,
                &Provenance::new(Author::Human, "T07 original"),
            )?;
            Ok("prd".into())
        })
        .unwrap();
        self.approve(prd);
        self.cmd(move |tx| {
            tx.put_review_document(adr, None, prd, None, "T07 ADR", "fixture reviewed")?;
            Ok("adr".into())
        })
        .unwrap();
        self.approve(adr);
        let mut p = plan(&(1..=repos.len() as u128).collect::<Vec<_>>(), vec![]);
        p.schema = 2;
        for (i, r) in repos.iter().enumerate() {
            p.inputs[i].base_sources[0] = Source {
                repository: r.id(),
                revision: SourceRevision::Commit(self.base.clone()),
            };
            p.resources.push(StepResources {
                step: node(i as u128 + 1),
                repository: r.id(),
                provider: if i % 2 == 0 {
                    ProviderKind::ClaudeCode
                } else {
                    ProviderKind::Codex
                },
                priority: 0,
                additional: vec![],
                repository_mode: ResourceMode::Shared,
            });
        }
        for a in [prd, adr] {
            let approval = self
                .e
                .reader()
                .unwrap()
                .read(|v| v.current_artifact_approval(a))
                .unwrap()
                .unwrap();
            p.artifacts.nodes.push(NodeRef {
                id: a.0,
                revision: approval.revision,
            });
            p.approvals.push(approval);
        }
        let copy = p.clone();
        self.cmd(move |tx| {
            tx.put_execution_plan(pid, None, prd, adr, &copy, "T07 reviewed")?;
            Ok("plan".into())
        })
        .unwrap();
        self.approve(pid);
        for (i, input) in p.inputs.into_iter().enumerate() {
            self.cmd(move |tx| {
                tx.queue_step(
                    prd,
                    adr,
                    pid,
                    Revision(1),
                    node(i as u128 + 1),
                    &SourceVector {
                        sources: input.base_sources,
                        environment: Some("test-env".into()),
                    },
                )?;
                Ok("queued".into())
            })
            .unwrap();
        }
        self.cmd(move |tx| {
            tx.configure_resources(
                1,
                cap,
                std::collections::BTreeMap::from([
                    ("global".into(), cap),
                    ("provider:claude".into(), cap),
                    ("provider:codex".into(), cap),
                ]),
            )?;
            tx.set_admission(Admission::Open)?;
            Ok("config".into())
        })
        .unwrap();
        2
    }
    fn reserve(&self, config: u64, ids: &[u128]) -> Vec<OpaqueId> {
        let proofs = ids
            .iter()
            .map(|a| inspect_worktree(&self.e, OpaqueId(*a)).unwrap())
            .collect();
        self.cmd(move |tx| {
            Ok(tx
                .schedule_owned_worktrees(config, proofs)?
                .iter()
                .filter_map(|d| d.attempt)
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>()
                .join(","))
        })
        .unwrap()
        .response
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| OpaqueId(s.parse().unwrap()))
        .collect()
    }
}
#[test]
fn t07_preprovisioned_same_repository_cap_two_separate_edits_and_outputs() {
    let f = F::new();
    let a = f.create(101);
    let b = f.create(102);
    let cfg = f.queued(&[f.repo.clone(), f.repo.clone()], 2);
    assert_eq!(f.reserve(cfg, &[101, 102]).len(), 2);
    fs::write(a.join("file"), "first").unwrap();
    fs::write(b.join("file"), "second").unwrap();
    assert_eq!(fs::read_to_string(a.join("file")).unwrap(), "first");
    assert_eq!(fs::read_to_string(b.join("file")).unwrap(), "second");
    assert_ne!(f.record(101).output, f.record(102).output);
    let claims = f.e.reader().unwrap().snapshot().unwrap().live_claims;
    assert_eq!(claims.len(), 10);
    assert!(collect_candidate(&f.e, OpaqueId(101)).is_err());
}
#[test]
fn t07_repeated_creation_preserves_dirty_work() {
    let f = F::new();
    let p = f.create(101);
    fs::write(p.join("file"), "keep").unwrap();
    assert_eq!(f.begin(101), "READY");
    assert!(materialize_worktree(&f.e, OpaqueId(101), None).is_err());
    assert_eq!(fs::read_to_string(p.join("file")).unwrap(), "keep");
}
#[test]
fn t07_linked_and_symlink_aliases_share_gate() {
    let f = F::new();
    let p = f.create(101);
    let alias = LocalRepository::inspect(&p).unwrap();
    let link = f.dir.path().join("alias");
    symlink(f.repo.path(), &link).unwrap();
    assert_eq!(LocalRepository::inspect(&link).unwrap().id(), f.repo.id());
    assert_eq!(alias.id(), f.repo.id());
    assert_eq!(f.begin_repo(102, alias, None).unwrap(), "CREATING");
    assert_eq!(f.begin(103), "WAITING");
}
#[test]
fn t07_independent_repositories_can_administer_simultaneously() {
    let f = F::new();
    let (r, _) = repo(&f.dir.path().join("second"));
    assert_eq!(f.begin(101), "CREATING");
    assert_eq!(f.begin_repo(102, r, None).unwrap(), "CREATING");
    assert_eq!(
        f.e.reader().unwrap().snapshot().unwrap().live_claims.len(),
        2
    );
}
#[test]
fn t07_admin_waiter_drains_shared_scheduler_claims() {
    let f = F::new();
    f.create(101);
    f.create(102);
    let cfg = f.queued(&[f.repo.clone(), f.repo.clone()], 2);
    assert_eq!(f.reserve(cfg, &[101]), vec![OpaqueId(101)]);
    assert_eq!(f.begin(103), "WAITING");
    assert!(f.reserve(cfg, &[102]).is_empty());
    f.cmd(|tx| {
        tx.retire_unissued_reservation(OpaqueId(101))?;
        Ok("retired".into())
    })
    .unwrap();
    assert_eq!(f.begin(103), "CREATING");
}
#[test]
fn t07_concurrent_administration_has_one_owner() {
    let f = Arc::new(F::new());
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [101, 102]
        .into_iter()
        .map(|id| {
            let f = f.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                f.begin(id)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|s| *s == "CREATING").count(), 1);
    assert_eq!(results.iter().filter(|s| *s == "WAITING").count(), 1);
}
#[test]
fn t07_existing_user_directory_is_not_adopted_or_deleted() {
    let f = F::new();
    assert_eq!(f.begin(101), "CREATING");
    let r = f.record(101);
    let parent = r.path.parent().unwrap();
    fs::create_dir(parent).unwrap();
    fs::write(parent.join("user"), "preserve").unwrap();
    assert!(materialize_worktree(&f.e, OpaqueId(101), None).is_err());
    f.cmd(|tx| {
        tx.block_worktree(OpaqueId(101), "existing path")?;
        Ok("blocked".into())
    })
    .unwrap();
    assert_eq!(f.record(101).state, "BLOCKED");
    assert_eq!(fs::read(parent.join("user")).unwrap(), b"preserve");
    assert_eq!(f.begin(102), "WAITING");
}
#[test]
fn t07_existing_branch_causes_preserved_partial_creation() {
    let f = F::new();
    git(f.repo.path(), &["branch", &format!("jojo/{:032x}", 101)]);
    assert_eq!(f.begin(101), "CREATING");
    assert!(materialize_worktree(&f.e, OpaqueId(101), None).is_err());
    let r = f.record(101);
    assert!(r.output.exists());
    assert!(r.path.parent().unwrap().join("owner").exists());
    assert!(materialize_worktree(&f.e, OpaqueId(101), None).is_err());
    assert_eq!(f.begin(102), "WAITING");
}
#[test]
fn t07_invalid_and_traversal_repository_paths_refused() {
    let f = F::new();
    for p in [
        PathBuf::from("relative"),
        f.dir.path().join("missing"),
        f.dir.path().join(".."),
        f.dir.path().to_path_buf(),
    ] {
        assert!(LocalRepository::inspect(&p).is_err());
    }
}
#[test]
fn t07_protected_paths_refused_even_through_alias() {
    let f = F::new();
    for p in [
        f.dir.path().join("agentic-factory"),
        f.dir.path().join("research/upstream/repo"),
    ] {
        fs::create_dir_all(&p).unwrap();
        git(&p, &["init", "--template="]);
        assert!(LocalRepository::inspect(&p).is_err());
        let link = f.dir.path().join(format!(
            "alias-{}",
            p.file_name().unwrap().to_str().unwrap()
        ));
        symlink(&p, &link).unwrap();
        assert!(LocalRepository::inspect(&link).is_err());
    }
}
#[test]
fn t07_symlink_attempt_directory_cannot_escape_owned_root() {
    let f = F::new();
    f.begin(101);
    let outside = f.dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let r = f.record(101);
    symlink(&outside, r.path.parent().unwrap()).unwrap();
    assert!(materialize_worktree(&f.e, OpaqueId(101), None).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
#[test]
fn t07_owned_root_replacement_refused() {
    let f = F::new();
    f.begin(101);
    let root = f
        .record(101)
        .path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    fs::rename(&root, root.with_extension("saved")).unwrap();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(materialize_worktree(&f.e, OpaqueId(101), None).is_err());
}
#[test]
fn t07_cross_attempt_marker_and_output_symlink_refused() {
    let f = F::new();
    f.create(101);
    f.create(102);
    let a = f.record(101);
    let b = f.record(102);
    fs::copy(
        a.path.parent().unwrap().join("owner"),
        b.path.parent().unwrap().join("owner"),
    )
    .unwrap();
    assert!(inspect_worktree(&f.e, OpaqueId(102)).is_err());
    fs::rename(&a.output, a.output.with_extension("saved")).unwrap();
    symlink(f.dir.path(), &a.output).unwrap();
    assert!(collect_candidate(&f.e, OpaqueId(101)).is_err());
}
#[test]
fn t07_dirty_staged_untracked_modes_symlinks_fork_faithfully() {
    let f = F::new();
    let p = f.create(101);
    fs::write(p.join("file"), "staged\n").unwrap();
    git(&p, &["add", "file"]);
    fs::write(p.join("file"), "unstaged\n").unwrap();
    fs::set_permissions(p.join("exec"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(p.join("untracked ; literal"), [0, 1, 255, 10]).unwrap();
    symlink("file", p.join("link")).unwrap();
    let c = f.candidate(101);
    let again = f.candidate(101);
    assert_eq!(c.digest(), again.digest());
    let reopened = CandidateSnapshot::reopen(c.path(), c.digest()).unwrap();
    assert_eq!(
        f.begin_repo(102, f.repo.clone(), Some(101)).unwrap(),
        "CREATING"
    );
    f.finish(102, Some(&reopened));
    let out = f.record(102);
    assert_ne!(out.path, p);
    assert_eq!(out.snapshot.as_deref(), Some(c.digest()));
    assert_eq!(fs::read(out.path.join("file")).unwrap(), b"unstaged\n");
    assert_eq!(git(&out.path, &["show", ":file"]), "staged");
    assert_eq!(
        fs::read(out.path.join("untracked ; literal")).unwrap(),
        [0, 1, 255, 10]
    );
    assert_eq!(
        fs::read_link(out.path.join("link")).unwrap(),
        Path::new("file")
    );
    assert_ne!(
        fs::metadata(out.path.join("exec"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert_eq!(fs::read(p.join("file")).unwrap(), b"unstaged\n");
    assert!(inspect_worktree(&f.e, OpaqueId(102)).is_ok());
}
#[test]
fn t07_tracked_symlink_and_staged_deletion_preserved() {
    let f = F::new();
    let p = f.create(101);
    symlink("exec", p.join("tracked-link")).unwrap();
    git(&p, &["add", "tracked-link"]);
    fs::remove_file(p.join("file")).unwrap();
    git(&p, &["add", "file"]);
    let c = f.candidate(101);
    f.begin_repo(102, f.repo.clone(), Some(101)).unwrap();
    f.finish(102, Some(&c));
    let q = f.record(102).path;
    assert_eq!(
        fs::read_link(q.join("tracked-link")).unwrap(),
        Path::new("exec")
    );
    assert!(!q.join("file").exists());
    assert_eq!(
        git(&p, &["diff", "--cached", "--binary"]),
        git(&q, &["diff", "--cached", "--binary"])
    );
}
#[test]
fn t07_ignored_files_block_snapshot_without_copying() {
    let f = F::new();
    let p = f.create(101);
    fs::write(p.join(".gitignore"), "secret\n").unwrap();
    fs::write(p.join("secret"), "local secret fixture").unwrap();
    assert!(
        collect_candidate(&f.e, OpaqueId(101))
            .unwrap_err()
            .contains("ignored")
    );
    assert_eq!(fs::read(p.join("secret")).unwrap(), b"local secret fixture");
}
#[test]
fn t07_external_symlink_targets_block_snapshot() {
    let f = F::new();
    let p = f.create(101);
    symlink("../../outside", p.join("link")).unwrap();
    assert!(collect_candidate(&f.e, OpaqueId(101)).is_err());
    fs::remove_file(p.join("link")).unwrap();
    symlink(f.dir.path(), p.join("absolute")).unwrap();
    assert!(collect_candidate(&f.e, OpaqueId(101)).is_err());
}
#[test]
fn t07_snapshot_tampering_and_symlink_packet_refused() {
    let f = F::new();
    f.create(101);
    let c = f.candidate(101);
    let alias = f.dir.path().join("packet-alias");
    symlink(c.path(), &alias).unwrap();
    assert!(CandidateSnapshot::reopen(&alias, c.digest()).is_err());
    fs::write(c.path(), "tampered").unwrap();
    assert!(CandidateSnapshot::reopen(c.path(), c.digest()).is_err());
    f.begin_repo(102, f.repo.clone(), Some(101)).unwrap();
    assert!(materialize_worktree(&f.e, OpaqueId(102), Some(&c)).is_err());
    assert!(!f.record(102).path.exists());
}
#[test]
fn t07_wrong_predecessor_candidate_refused_before_creation() {
    let f = F::new();
    f.create(101);
    f.create(102);
    let c = f.candidate(101);
    f.begin_repo(103, f.repo.clone(), Some(102)).unwrap();
    assert!(materialize_worktree(&f.e, OpaqueId(103), Some(&c)).is_err());
    assert!(!f.record(103).output.exists());
}
#[test]
fn t07_changed_tree_and_head_block_execution_inspection() {
    let f = F::new();
    let p = f.create(101);
    fs::write(p.join("file"), "changed").unwrap();
    assert!(inspect_worktree(&f.e, OpaqueId(101)).is_err());
    git(&p, &["add", "file"]);
    git(&p, &["commit", "-m", "candidate"]);
    assert!(inspect_worktree(&f.e, OpaqueId(101)).is_err());
    let c = f.candidate(101);
    assert_eq!(c.head(), git(&p, &["rev-parse", "HEAD"]));
}
#[test]
fn t07_reservation_is_not_worktree_or_ownership_proof() {
    let f = F::new();
    let p = f.create(101);
    let inspected = inspect_worktree(&f.e, OpaqueId(101)).unwrap();
    assert!(
        f.cmd(move |tx| {
            tx.bind_worktree_execution(inspected)?;
            Ok("bad".into())
        })
        .is_err()
    );
    assert_eq!(fs::read(p.join("file")).unwrap(), b"base\n");
    assert!(inspect_worktree(&f.e, OpaqueId(999)).is_err());
}
#[test]
fn t07_wrong_base_cannot_bind_reserved_source() {
    let f = F::new();
    let root = f.root.clone();
    let repo = f.repo.clone();
    fs::write(repo.path().join("file"), "new base").unwrap();
    git(repo.path(), &["add", "file"]);
    git(repo.path(), &["commit", "-m", "other base"]);
    let base = git(repo.path(), &["rev-parse", "HEAD"]);
    f.cmd(move |tx| tx.request_worktree(OpaqueId(101), &repo, &root, &base, None))
        .unwrap();
    f.finish(101, None);
    let cfg = f.queued(std::slice::from_ref(&f.repo), 1);
    let proof = inspect_worktree(&f.e, OpaqueId(101)).unwrap();
    assert!(
        f.cmd(move |tx| {
            tx.schedule_owned_worktrees(cfg, vec![proof])?;
            Ok("bad".into())
        })
        .is_err()
    );
    assert!(
        f.e.reader()
            .unwrap()
            .snapshot()
            .unwrap()
            .live_claims
            .is_empty()
    );
}
#[test]
fn t07_restart_preserves_dirty_and_blocks_old_epoch_reuse() {
    let f = F::new();
    let p = f.create(101);
    fs::write(p.join("file"), "interrupted").unwrap();
    let c = f.candidate(101);
    let old = f.record(101);
    let F {
        dir, e, _serial, ..
    } = f;
    drop(e);
    let e = start(&dir.db());
    assert!(e.epoch() > old.epoch);
    assert_eq!(
        e.reader().unwrap().snapshot().unwrap().session.admission,
        Admission::Closed
    );
    assert!(inspect_worktree(&e, OpaqueId(101)).is_err());
    assert!(observe_worktree(&old).is_ok());
    assert_eq!(fs::read(p.join("file")).unwrap(), b"interrupted");
    assert_eq!(
        CandidateSnapshot::reopen(c.path(), c.digest())
            .unwrap()
            .digest(),
        c.digest()
    );
}
#[test]
fn t07_restart_partial_create_quarantines_and_never_replays() {
    let f = F::new();
    f.begin(101);
    let proof = materialize_worktree(&f.e, OpaqueId(101), None).unwrap();
    drop(proof);
    let old = f.record(101);
    let F {
        dir, e, _serial, ..
    } = f;
    drop(e);
    let e = start(&dir.db());
    assert!(materialize_worktree(&e, OpaqueId(101), None).is_err());
    assert_eq!(
        e.reader()
            .unwrap()
            .read(|v| v.worktree(OpaqueId(101)))
            .unwrap()
            .unwrap()
            .state,
        "CREATING"
    );
    assert_eq!(
        e.reader().unwrap().snapshot().unwrap().live_claims[0].state,
        ClaimState::Quarantined
    );
    assert!(observe_worktree(&old).is_ok());
}
#[test]
fn t07_executable_repository_filters_are_refused() {
    let f = F::new();
    git(
        f.repo.path(),
        &["config", "filter.fixture.smudge", "/not/executed"],
    );
    assert!(LocalRepository::inspect(f.repo.path()).is_err());
}
#[test]
fn t07_creation_does_not_modify_user_checkout_or_remote_config() {
    let f = F::new();
    git(
        f.repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/not-contacted",
        ],
    );
    fs::write(f.repo.path().join("file"), "user unstaged").unwrap();
    let before = git(f.repo.path(), &["status", "--porcelain=v1"]);
    let config = fs::read(f.repo.path().join(".git/config")).unwrap();
    f.create(101);
    assert_eq!(git(f.repo.path(), &["status", "--porcelain=v1"]), before);
    assert_eq!(fs::read(f.repo.path().join(".git/config")).unwrap(), config);
    assert_eq!(
        fs::read(f.repo.path().join("file")).unwrap(),
        b"user unstaged"
    );
}
#[test]
fn t07_independent_repositories_execution_overlaps_with_correct_bindings() {
    let f = F::new();
    f.create(101);
    let (second, base) = repo(&f.dir.path().join("independent"));
    assert_eq!(base, f.base);
    f.begin_repo(102, second.clone(), None).unwrap();
    f.finish(102, None);
    let cfg = f.queued(&[f.repo.clone(), second], 2);
    let admitted = f.reserve(cfg, &[102, 101]);
    assert_eq!(admitted.len(), 2);
    assert_eq!(f.record(101).state, "BOUND");
    assert_eq!(f.record(102).state, "BOUND");
}
#[test]
fn t07_cap_one_defers_preprovisioned_second_worktree() {
    let f = F::new();
    f.create(101);
    f.create(102);
    let cfg = f.queued(&[f.repo.clone(), f.repo.clone()], 1);
    assert_eq!(f.reserve(cfg, &[101, 102]).len(), 1);
    assert_eq!(f.record(102).state, "READY");
    assert!(f.reserve(cfg, &[102]).is_empty());
}
#[test]
fn t07_stale_epoch_materialization_proof_cannot_seal() {
    let f = F::new();
    f.begin(101);
    let proof = materialize_worktree(&f.e, OpaqueId(101), None).unwrap();
    let F {
        dir, e, _serial, ..
    } = f;
    drop(e);
    let e = start(&dir.db());
    assert!(
        e.execute(request(&e, 1000, "stale proof", "T07"), move |tx| {
            tx.seal_worktree(proof)?;
            Ok("bad".into())
        })
        .is_err()
    );
    assert_eq!(
        e.reader()
            .unwrap()
            .read(|v| v.worktree(OpaqueId(101)))
            .unwrap()
            .unwrap()
            .state,
        "CREATING"
    );
}
#[test]
fn t07_attempt_identity_cannot_be_reassigned_to_another_repository() {
    let f = F::new();
    f.create(101);
    let (other, _) = repo(&f.dir.path().join("other"));
    assert!(f.begin_repo(101, other, None).is_err());
    assert_eq!(f.record(101).repository, f.repo.id());
}
#[test]
fn t07_replaced_common_directory_refused() {
    let f = F::new();
    f.begin(101);
    let common = f.repo.path().join(".git");
    fs::rename(&common, f.repo.path().join(".git-saved")).unwrap();
    git(
        f.repo.path(),
        &["init", "--template=", "--initial-branch=main"],
    );
    assert!(materialize_worktree(&f.e, OpaqueId(101), None).is_err());
}
#[test]
fn t07_committed_candidate_forks_exact_commit_without_changing_original() {
    let f = F::new();
    let p = f.create(101);
    fs::write(p.join("file"), "candidate commit").unwrap();
    git(&p, &["add", "file"]);
    git(&p, &["commit", "-m", "candidate"]);
    let c = f.candidate(101);
    let repo = f.repo.clone();
    let root = f.root.clone();
    let base = c.head().to_owned();
    f.cmd(move |tx| tx.request_worktree(OpaqueId(102), &repo, &root, &base, Some(OpaqueId(101))))
        .unwrap();
    f.finish(102, Some(&c));
    assert_eq!(git(&f.record(102).path, &["rev-parse", "HEAD"]), c.head());
    assert_eq!(git(&p, &["rev-parse", "HEAD"]), c.head());
    assert_ne!(f.record(101).branch, f.record(102).branch);
}
#[test]
fn t07_binary_tracked_changes_survive_snapshot_materialization() {
    let f = F::new();
    let p = f.create(101);
    fs::write(p.join("file"), [0, 255, 1, 10]).unwrap();
    git(&p, &["add", "file"]);
    fs::write(p.join("file"), [0, 128, 2, 10]).unwrap();
    let c = f.candidate(101);
    f.begin_repo(102, f.repo.clone(), Some(101)).unwrap();
    f.finish(102, Some(&c));
    assert_eq!(
        fs::read(f.record(102).path.join("file")).unwrap(),
        [0, 128, 2, 10]
    );
    assert_eq!(
        git(&p, &["diff", "--cached", "--binary"]),
        git(&f.record(102).path, &["diff", "--cached", "--binary"])
    );
}
#[test]
fn t07_unrepresented_index_flags_block_snapshot() {
    let f = F::new();
    let p = f.create(101);
    git(&p, &["update-index", "--assume-unchanged", "file"]);
    assert!(
        collect_candidate(&f.e, OpaqueId(101))
            .unwrap_err()
            .contains("index flags")
    );
}
#[test]
fn t07_candidate_registry_is_idempotent_and_durable() {
    let f = F::new();
    f.create(101);
    let c = f.candidate(101);
    let before = f.e.reader().unwrap().snapshot().unwrap().as_of_event_seq;
    let again = f.candidate(101);
    assert_eq!(c.digest(), again.digest());
    assert_eq!(
        f.e.reader().unwrap().snapshot().unwrap().as_of_event_seq,
        before
    );
    let reference =
        f.e.reader()
            .unwrap()
            .read(|v| v.candidate_reference(OpaqueId(101), c.digest()))
            .unwrap()
            .unwrap();
    assert_eq!(reference.path, c.path());
    let F {
        dir, e, _serial, ..
    } = f;
    drop(e);
    let e = start(&dir.db());
    let persisted = e
        .reader()
        .unwrap()
        .read(|v| v.candidate_reference(OpaqueId(101), &reference.digest))
        .unwrap()
        .unwrap();
    assert_eq!(persisted, reference);
    assert!(CandidateSnapshot::reopen(&persisted.path, &persisted.digest).is_ok());
}
#[test]
fn t07_scheduler_reservation_cannot_upgrade_into_administration() {
    let f = F::new();
    let cfg = f.queued(std::slice::from_ref(&f.repo), 1);
    f.cmd(move |tx| {
        assert_eq!(
            tx.schedule(cfg, &[OpaqueId(101)])?
                .iter()
                .filter(|d| d.attempt.is_some())
                .count(),
            1
        );
        Ok("reserved".into())
    })
    .unwrap();
    assert!(f.begin_repo(101, f.repo.clone(), None).is_err());
    assert!(
        f.e.reader()
            .unwrap()
            .read(|v| v.worktree(OpaqueId(101)))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.e.reader().unwrap().snapshot().unwrap().live_claims.len(),
        3
    );
}
#[test]
fn t07_submodule_contents_block_instead_of_being_omitted() {
    let f = F::new();
    let p = f.create(101);
    git(
        &p,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},module", f.base),
        ],
    );
    assert!(
        collect_candidate(&f.e, OpaqueId(101))
            .unwrap_err()
            .contains("submodule")
    );
    assert!(git(&p, &["ls-files", "--stage"]).contains("160000"));
}
#[test]
fn t07_reopened_packets_refuse_traversal_and_external_symlinks() {
    let f = F::new();
    f.create(101);
    let c = f.candidate(101);
    let original = fs::read(c.path()).unwrap();
    for (index, name, bytes, link) in [
        (0, "../escape", b"data".as_slice(), false),
        (1, "link", b"../../escape".as_slice(), true),
        (2, ".git/config", b"data".as_slice(), false),
    ] {
        let mut packet = original.clone();
        for field in [
            name.as_bytes().to_vec(),
            bytes.to_vec(),
            0o644u32.to_be_bytes().to_vec(),
            vec![u8::from(link)],
        ] {
            packet.extend_from_slice(&(field.len() as u64).to_be_bytes());
            packet.extend(field);
        }
        let digest = sha256_hex(&[&packet]);
        let file = f.dir.path().join(format!("packet-{index}"));
        fs::write(&file, packet).unwrap();
        assert!(CandidateSnapshot::reopen(&file, &digest).is_err());
    }
    assert!(!f.dir.path().join("escape").exists());
}
