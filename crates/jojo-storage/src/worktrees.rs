//! Local Git administration. Filesystem effects are deliberately outside writer transactions.
//! Tickets retain their exclusive claim across ambiguity; restart never replays them.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use crate::model::{id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::{
    AggregateKey, ClaimId, ClaimMode, ClaimState, Engine, EngineEpoch, ReadView, ReleaseBasis,
    StorageError, Tx, TxError, sha256_hex,
};
use jojo_domain::identity::{OpaqueId, RepositoryId};
use jojo_domain::scheduler::repository_key;
use jojo_domain::validation::{ChangeKind, ChangedPath, ScopePath, parse_raw_diff};

type IoResult<T> = Result<T, String>;
fn io<T>(r: std::io::Result<T>) -> IoResult<T> {
    r.map_err(|e| e.to_string())
}
fn deny(s: &str) -> TxError {
    TxError::Rejected(s.into())
}
fn text_path(p: &Path) -> IoResult<&str> {
    p.to_str().ok_or("non-UTF8 path is unsupported".into())
}
fn identity(p: &Path) -> IoResult<String> {
    let m = io(fs::metadata(p))?;
    Ok(format!("{}:{}", m.dev(), m.ino()))
}
fn safe_input(p: &Path) -> IoResult<()> {
    if !p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("absolute path without traversal required".into());
    }
    if p.components().any(|c| c.as_os_str() == "agentic-factory")
        || p.components()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w[0].as_os_str() == "research" && w[1].as_os_str() == "upstream")
    {
        return Err("protected repository path".into());
    }
    Ok(())
}
fn canonical(p: &Path) -> IoResult<PathBuf> {
    safe_input(p)?;
    let p = io(p.canonicalize())?;
    safe_input(&p)?;
    Ok(p)
}

/// A closed local argv surface. No shell, inherited Git environment, hooks, network or providers.
fn git(cwd: &Path, args: &[&str], input: Option<&[u8]>) -> IoResult<Vec<u8>> {
    if !matches!(
        args.first().copied(),
        Some(
            "rev-parse"
                | "config"
                | "worktree"
                | "symbolic-ref"
                | "diff"
                | "ls-files"
                | "apply"
                | "clone"
                | "checkout"
        )
    ) || (args.first() == Some(&"worktree") && args.get(1) != Some(&"add"))
        || (args.first() == Some(&"checkout") && args.get(1) != Some(&"--detach"))
    {
        return Err("unsupported local Git command".into());
    }
    // P2-03: the validation copy is cloned from the registered local path. Only that
    // transport is opened, for that one command; every other protocol stays refused.
    let transport: &[&str] = if args.first() == Some(&"clone") {
        &["-c", "protocol.file.allow=always"]
    } else {
        &[]
    };
    let mut child = Command::new("/usr/bin/git")
        .env_clear()
        .envs([
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_OPTIONAL_LOCKS", "0"),
            ("LC_ALL", "C"),
        ])
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "protocol.allow=never",
            "-c",
            "submodule.recurse=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(transport)
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(bytes) = input {
        io(child.stdin.take().ok_or("missing stdin")?.write_all(bytes))?;
    }
    let out = io(child.wait_with_output())?;
    if !out.status.success() {
        return Err(format!(
            "local Git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    if out.stdout.len() > 32 * 1024 * 1024 {
        return Err("Git output exceeds snapshot bound".into());
    }
    Ok(out.stdout)
}
fn line(bytes: Vec<u8>) -> IoResult<String> {
    String::from_utf8(bytes)
        .map(|s| s.trim_end_matches('\n').to_owned())
        .map_err(|_| "non-UTF8 Git identity".into())
}
fn oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalRepository {
    path: PathBuf,
    common: PathBuf,
    filesystem: String,
    id: RepositoryId,
}
impl LocalRepository {
    pub fn inspect(path: &Path) -> IoResult<Self> {
        let path = canonical(path)?;
        let common = canonical(Path::new(&line(git(
            &path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            None,
        )?)?))?;
        // Checkout filters can run arbitrary local programs; reject rather than inheriting them.
        let config = line(git(&path, &["config", "--name-only", "--list"], None)?)?;
        if config
            .lines()
            .any(|s| s.starts_with("filter.") || s.starts_with("include") || s == "core.sshcommand")
        {
            return Err("unsupported executable/included repository configuration".into());
        }
        let filesystem = identity(&common)?;
        let hash = sha256_hex(&[text_path(&common)?.as_bytes(), filesystem.as_bytes()]);
        let id = RepositoryId(OpaqueId(
            u128::from_str_radix(&hash[..32], 16).map_err(|e| e.to_string())?,
        ));
        Ok(Self {
            path,
            common,
            filesystem,
            id,
        })
    }
    pub fn id(&self) -> RepositoryId {
        self.id
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn common(&self) -> &Path {
        &self.common
    }
    fn recheck(&self) -> IoResult<()> {
        if Self::inspect(&self.path)? != *self {
            Err("repository identity changed".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeRoot {
    path: PathBuf,
    filesystem: String,
}
impl WorktreeRoot {
    /// Caller creates a private directory. Existing user directories are never adopted.
    pub fn inspect(path: &Path) -> IoResult<Self> {
        let path = canonical(path)?;
        let m = io(fs::symlink_metadata(&path))?;
        if !m.is_dir() || m.permissions().mode() & 0o077 != 0 {
            return Err("owned root must be a private directory (0700)".into());
        }
        Ok(Self {
            filesystem: identity(&path)?,
            path,
        })
    }
    fn recheck(&self) -> IoResult<()> {
        if Self::inspect(&self.path)? != *self {
            Err("owned root replaced".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeRecord {
    pub attempt: OpaqueId,
    pub repository: RepositoryId,
    pub epoch: EngineEpoch,
    pub state: String,
    pub path: PathBuf,
    pub output: PathBuf,
    pub branch: String,
    pub base: String,
    pub snapshot: Option<String>,
    pub predecessor: Option<OpaqueId>,
    root: WorktreeRoot,
    repo: LocalRepository,
    claim: Option<ClaimId>,
    marker: String,
}
impl WorktreeRecord {
    /// The immutable repository identity path recorded when this worktree was owned.
    pub fn common_dir(&self) -> &Path {
        &self.repo.common
    }
}

pub(crate) fn load(
    c: &Connection,
    attempt: OpaqueId,
) -> Result<Option<WorktreeRecord>, StorageError> {
    c.query_optional("SELECT repository,epoch,state,root,root_identity,repo_path,common,common_identity,base,predecessor,claim,marker,snapshot FROM owned_worktrees WHERE attempt=?1",&[Value::Text(&id_to_text(attempt))],|r| {
        let rid=r.text(0)?; let epoch=r.int(1)?; let root=PathBuf::from(r.text(3)?); let pred=r.optional_text(9)?;
        Ok((rid,epoch,r.text(2)?,root,r.text(4)?,r.text(5)?,r.text(6)?,r.text(7)?,r.text(8)?,pred,r.optional_int(10)?,r.text(11)?,r.optional_text(12)?))
    })?.map(|(rid,epoch,state,root,root_identity,repo_path,common,common_identity,base,pred,claim,marker,snapshot)| {
        let parse=|s:&str|u128::from_str_radix(s,16).map(OpaqueId).map_err(|_|StorageError::corrupt("invalid worktree identity"));
        let repository=RepositoryId(parse(&rid)?); let dir=root.join(id_to_text(attempt));
        Ok::<_, StorageError>(WorktreeRecord{attempt,repository,epoch:EngineEpoch(epoch as u64),state,path:dir.join("tree"),output:dir.join("output"),branch:format!("jojo/{}",id_to_text(attempt)),base,snapshot,predecessor:pred.as_deref().map(parse).transpose()?,root:WorktreeRoot{path:root,filesystem:root_identity},repo:LocalRepository{path:PathBuf::from(repo_path),common:PathBuf::from(common),filesystem:common_identity,id:repository},claim:claim.map(|v|ClaimId(v as u64)),marker})
    }).transpose()
}
impl ReadView<'_> {
    pub fn worktree(&self, attempt: OpaqueId) -> Result<Option<WorktreeRecord>, StorageError> {
        load(self.conn, attempt)
    }
}

impl Tx<'_> {
    pub fn register_local_repository(
        &mut self,
        repo: &LocalRepository,
    ) -> Result<RepositoryId, TxError> {
        if let Some((common, filesystem)) = self.conn.query_optional(
            "SELECT common,filesystem FROM local_repositories WHERE id=?1",
            &[Value::Text(&id_to_text(repo.id.0))],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )? {
            if common != text_path(&repo.common).map_err(|e| deny(&e))?
                || filesystem != repo.filesystem
            {
                return Err(deny("repository registration collision"));
            }
            return Ok(repo.id);
        }
        self.conn.execute(
            "INSERT INTO local_repositories(id,path,common,filesystem) VALUES (?1,?2,?3,?4)",
            &[
                Value::Text(&id_to_text(repo.id.0)),
                Value::Text(text_path(&repo.path).map_err(|e| deny(&e))?),
                Value::Text(text_path(&repo.common).map_err(|e| deny(&e))?),
                Value::Text(&repo.filesystem),
            ],
        )?;
        self.append_event("LocalRepositoryRegistered", None, &id_to_text(repo.id.0))?;
        Ok(repo.id)
    }
    pub fn record_candidate(&mut self, candidate: &CandidateSnapshot) -> Result<(), TxError> {
        let r =
            load(self.conn, candidate.attempt)?.ok_or_else(|| deny("candidate attempt missing"))?;
        let recovered =
            crate::recovery::recovered_absent(self.conn, &id_to_text(candidate.attempt))?;
        if (r.epoch != self.epoch && !recovered)
            || r.repository != candidate.repository
            || candidate.file != r.output.join(format!("candidate-{}", candidate.digest))
            || !matches!(r.state.as_str(), "READY" | "BOUND")
        {
            return Err(deny("candidate ownership conflict"));
        }
        if !self
            .live_claims(&format!("worktree:{}", id_to_text(r.attempt)))?
            .is_empty()
        {
            return Err(deny("candidate writer is active or uncertain"));
        }
        let changed=self.conn.execute("INSERT OR IGNORE INTO worktree_candidates(attempt,digest,head,path) VALUES(?1,?2,?3,?4)",&[Value::Text(&id_to_text(candidate.attempt)),Value::Text(&candidate.digest),Value::Text(&candidate.head),Value::Text(text_path(&candidate.file).map_err(|e|deny(&e))?)])?;
        if changed == 1 {
            self.append_event(
                "CandidatePreserved",
                None,
                &format!(
                    "{}|{}|{}",
                    id_to_text(candidate.attempt),
                    candidate.head,
                    candidate.digest
                ),
            )?;
        }
        Ok(())
    }
    /// Persists an exclusive administration waiter BEFORE provisioning. No partial holds.
    /// Calling again services a waiter, or returns the original sealed binding.
    pub fn request_worktree(
        &mut self,
        attempt: OpaqueId,
        repo: &LocalRepository,
        root: &WorktreeRoot,
        base: &str,
        predecessor: Option<OpaqueId>,
    ) -> Result<String, TxError> {
        self.register_local_repository(repo)?;
        if !oid(base) {
            return Err(deny("exact source commit required"));
        }
        if let Some(old) = load(self.conn, attempt)? {
            if old.repo != *repo
                || old.root != *root
                || old.base != base
                || old.predecessor != predecessor
                || old.epoch != self.epoch
            {
                return Err(deny("worktree identity/epoch conflict"));
            }
            if old.state != "WAITING" {
                return Ok(old.state);
            }
        } else {
            if self
                .live_claims(&repository_key(repo.id))?
                .iter()
                .any(|c| c.holder.kind == "scheduler_attempt" && c.holder.id == attempt)
            {
                return Err(deny(
                    "provision before acquiring execution claims; no shared-to-exclusive upgrade",
                ));
            }
            let marker = format!(
                "jojo-worktree-v1|{}|{}|{}|{}|{}",
                id_to_text(attempt),
                self.epoch.0,
                id_to_text(repo.id.0),
                base,
                text_path(&root.path).map_err(|e| deny(&e))?
            );
            self.conn.execute("INSERT INTO owned_worktrees(attempt,repository,epoch,state,root,root_identity,repo_path,common,common_identity,base,predecessor,marker) VALUES(?1,?2,?3,'WAITING',?4,?5,?6,?7,?8,?9,?10,?11)",&[
                Value::Text(&id_to_text(attempt)),Value::Text(&id_to_text(repo.id.0)),Value::Int(to_db(self.epoch.0)?),Value::Text(text_path(&root.path).map_err(|e|deny(&e))?),Value::Text(&root.filesystem),Value::Text(text_path(&repo.path).map_err(|e|deny(&e))?),Value::Text(text_path(&repo.common).map_err(|e|deny(&e))?),Value::Text(&repo.filesystem),Value::Text(base),predecessor.map(id_to_text).as_deref().map_or(Value::Null,Value::Text),Value::Text(&marker)])?;
            self.append_event("WorktreeRequested", None, &marker)?;
        }
        let key = repository_key(repo.id);
        if !self.live_claims(&key)?.is_empty() {
            return Ok("WAITING".into());
        }
        let holder = AggregateKey::new("worktree_admin", attempt);
        self.create_aggregate(&holder, "CREATING")?;
        let claim = self.acquire_claim(&key, ClaimMode::Exclusive, &holder)?;
        self.conn.execute(
            "UPDATE owned_worktrees SET state='CREATING',claim=?1 WHERE attempt=?2",
            &[
                Value::Int(to_db(claim.0)?),
                Value::Text(&id_to_text(attempt)),
            ],
        )?;
        self.append_event("WorktreeCreating", None, &id_to_text(attempt))?;
        Ok("CREATING".into())
    }
    pub fn seal_worktree(&mut self, proof: MaterializedWorktree) -> Result<(), TxError> {
        let record =
            load(self.conn, proof.record.attempt)?.ok_or_else(|| deny("missing worktree"))?;
        if record != proof.record || record.epoch != self.epoch || record.state != "CREATING" {
            return Err(deny("stale materialization"));
        }
        let claim = record
            .claim
            .ok_or_else(|| deny("missing administration claim"))?;
        if !self
            .live_claims(&repository_key(record.repository))?
            .iter()
            .any(|c| c.id == claim && c.epoch == self.epoch && c.state == ClaimState::Active)
        {
            return Err(deny("administration ownership lost"));
        }
        self.conn.execute(
            "UPDATE owned_worktrees SET state='READY',snapshot=?2 WHERE attempt=?1",
            &[
                Value::Text(&id_to_text(record.attempt)),
                proof.snapshot.as_deref().map_or(Value::Null, Value::Text),
            ],
        )?;
        self.release_claim(claim, ReleaseBasis::HolderSettled)?;
        self.append_event("WorktreeReady", None, &id_to_text(record.attempt))?;
        Ok(())
    }
    /// An ambiguous/failed create retains its exclusive claim and every filesystem byte.
    pub fn block_worktree(&mut self, attempt: OpaqueId, reason: &str) -> Result<(), TxError> {
        let r = load(self.conn, attempt)?.ok_or_else(|| deny("missing worktree"))?;
        if r.epoch != self.epoch || r.state != "CREATING" {
            return Err(deny("stale lifecycle"));
        }
        self.conn.execute(
            "UPDATE owned_worktrees SET state='BLOCKED' WHERE attempt=?1",
            &[Value::Text(&id_to_text(attempt))],
        )?;
        self.append_event("WorktreeBlocked", None, reason)?;
        Ok(())
    }
    /// Execution must claim both private writers in the scheduler reservation transaction.
    /// This does not infer filesystem safety from a reservation.
    pub fn bind_worktree_execution(&mut self, inspected: InspectedWorktree) -> Result<(), TxError> {
        let r =
            load(self.conn, inspected.record.attempt)?.ok_or_else(|| deny("missing binding"))?;
        if r != inspected.record || r.epoch != self.epoch || r.state != "READY" {
            return Err(deny("worktree not current and ready"));
        }
        let sources = self
            .conn
            .query_optional(
                "SELECT sources FROM dependency_attempt_inputs WHERE attempt=?1",
                &[Value::Text(&id_to_text(r.attempt))],
                |v| v.text(0),
            )?
            .ok_or_else(|| deny("frozen scheduler input required"))?;
        let sources = jojo_domain::dependency::SourceVector::import(sources.as_bytes())
            .map_err(|e| deny(&e))?;
        let expected = match &r.snapshot {
            Some(d) => jojo_domain::dependency::SourceRevision::Snapshot {
                base_commit: r.base.clone(),
                digest: d.clone(),
            },
            None => jojo_domain::dependency::SourceRevision::Commit(r.base.clone()),
        };
        // A reviewed continuation of the same step materializes the predecessor's sealed
        // candidate on top of the step's frozen source; it must match the plan exactly.
        let continuation = match r.predecessor {
            Some(p) => crate::recovery::continuation_binding(self.conn, r.attempt, p)?,
            None => None,
        };
        let matches = match &continuation {
            Some(c) => {
                r.snapshot == c.candidate
                    && r.base == c.base
                    && sources.canonical() == c.predecessor_sources
            }
            None => sources.sources.first().map(|s| &s.revision) == Some(&expected),
        };
        if sources.sources.len() != 1 || sources.sources[0].repository != r.repository || !matches {
            return Err(deny("owned worktree differs from frozen scheduler source"));
        }
        let holder = AggregateKey::new("scheduler_attempt", r.attempt);
        if !self
            .live_claims(&repository_key(r.repository))?
            .iter()
            .any(|c| c.holder == holder && c.epoch == self.epoch && c.state == ClaimState::Active)
        {
            return Err(deny("matching scheduler repository reservation required"));
        }
        for key in [
            format!("worktree:{}", id_to_text(r.attempt)),
            format!("output:{}", id_to_text(r.attempt)),
        ] {
            self.acquire_claim(&key, ClaimMode::Exclusive, &holder)?;
        }
        self.conn.execute(
            "UPDATE owned_worktrees SET state='BOUND' WHERE attempt=?1",
            &[Value::Text(&id_to_text(r.attempt))],
        )?;
        self.append_event("WorktreeBound", None, &id_to_text(r.attempt))?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct MaterializedWorktree {
    record: WorktreeRecord,
    snapshot: Option<String>,
}
#[derive(Debug)]
pub struct InspectedWorktree {
    record: WorktreeRecord,
}
fn current(engine: &Engine, attempt: OpaqueId) -> IoResult<WorktreeRecord> {
    let r = engine
        .reader()
        .map_err(|e| e.to_string())?
        .read(|v| v.worktree(attempt))
        .map_err(|e| e.to_string())?
        .ok_or("worktree missing")?;
    if r.epoch != engine.epoch() {
        return Err("old epoch requires recovery inspection; no automatic reuse".into());
    }
    r.root.recheck()?;
    r.repo.recheck()?;
    Ok(r)
}
/// Like [`current`], but also admits an earlier-epoch worktree whose writer this epoch's
/// recovery inspection proved absent. Only read-only capture/inspection uses this.
fn current_or_recovered(engine: &Engine, attempt: OpaqueId) -> IoResult<WorktreeRecord> {
    let (r, recovered, sealed) = engine
        .reader()
        .map_err(|e| e.to_string())?
        .read(|v| {
            Ok((
                v.worktree(attempt)?,
                crate::recovery::recovered_absent(v.conn, &id_to_text(attempt))?,
                v.attempt_result(attempt)?.is_some(),
            ))
        })
        .map_err(|e| e.to_string())?;
    let r = r.ok_or("worktree missing")?;
    // P2-03: a sealed result exists only after the attempt's process tree was proven
    // absent (schema trigger `attempt_result_proof`), so its retained worktree may be read
    // in a later epoch as well. Reading never modifies it and every reader re-checks the
    // sealed digest.
    if r.epoch != engine.epoch() && !recovered && !sealed {
        return Err("old epoch requires recovery inspection; no automatic reuse".into());
    }
    r.root.recheck()?;
    r.repo.recheck()?;
    Ok(r)
}
fn private_dir(p: &Path) -> IoResult<()> {
    io(fs::create_dir(p))?;
    io(fs::set_permissions(p, fs::Permissions::from_mode(0o700)))
}
fn write_new(p: &Path, bytes: &[u8]) -> IoResult<()> {
    let mut f = io(OpenOptions::new().write(true).create_new(true).open(p))?;
    io(f.write_all(bytes))?;
    io(f.sync_all())
}
fn marker_check(r: &WorktreeRecord) -> IoResult<()> {
    for p in [
        r.path.parent().ok_or("missing attempt parent")?,
        &r.path,
        &r.output,
    ] {
        if io(fs::symlink_metadata(p))?.file_type().is_symlink() || canonical(p)? != p {
            return Err("owned path replaced or symlinked".into());
        }
    }
    let p = r.path.parent().ok_or("missing parent")?.join("owner");
    if io(fs::symlink_metadata(&p))?.file_type().is_symlink()
        || io(fs::read(&p))? != r.marker.as_bytes()
    {
        return Err("ownership marker mismatch".into());
    }
    let gitfile = r.path.join(".git");
    if !io(fs::symlink_metadata(&gitfile))?.is_file() {
        return Err("worktree gitfile must be a regular file".into());
    }
    let private_gitdir = canonical(Path::new(&line(git(
        &r.path,
        &["rev-parse", "--absolute-git-dir"],
        None,
    )?)?))?;
    let reciprocal = line(io(fs::read(private_gitdir.join("gitdir")))?)?;
    if Path::new(&reciprocal) != gitfile {
        return Err("worktree gitdir belongs to another path".into());
    }
    let actual = LocalRepository::inspect(&r.path)?;
    if actual.common != r.repo.common || actual.filesystem != r.repo.filesystem {
        return Err("worktree common-dir mismatch".into());
    }
    if line(git(&r.path, &["symbolic-ref", "--quiet", "HEAD"], None)?)?
        != format!("refs/heads/{}", r.branch)
    {
        return Err("worktree branch mismatch".into());
    }
    Ok(())
}
/// Never retries a partial create. Inspectable partial directories and claims remain held.
/// `engine` keeps the OS singleton alive throughout the filesystem boundary.
pub fn materialize_worktree(
    engine: &Engine,
    attempt: OpaqueId,
    candidate: Option<&CandidateSnapshot>,
) -> IoResult<MaterializedWorktree> {
    let r = current(engine, attempt)?;
    if r.state != "CREATING" {
        return Err("exclusive CREATING ticket required".into());
    }
    let claims = engine
        .reader()
        .map_err(|e| e.to_string())?
        .read(|v| v.live_claims())
        .map_err(|e| e.to_string())?;
    if !claims.iter().any(|c| {
        Some(c.id) == r.claim && c.epoch == engine.epoch() && c.state == ClaimState::Active
    }) {
        return Err("administration claim not active".into());
    }
    if let Some(c) = candidate {
        c.verify()?;
        let registered = engine
            .reader()
            .map_err(|e| e.to_string())?
            .read(|v| v.candidate_reference(c.attempt, &c.digest))
            .map_err(|e| e.to_string())?
            .ok_or("candidate is not durably recorded")?;
        if registered.path != c.file || registered.head != c.head {
            return Err("candidate registry mismatch".into());
        }
        if r.predecessor != Some(c.attempt) || r.repository != c.repository || r.base != c.head {
            return Err("continuation candidate binding mismatch".into());
        }
    } else if r.predecessor.is_some() {
        return Err("preserved candidate required for continuation".into());
    }
    let head = line(git(
        &r.repo.path,
        &["rev-parse", "--verify", &format!("{}^{{commit}}", r.base)],
        None,
    )?)?;
    if head != r.base {
        return Err("base is not exact commit".into());
    }
    let parent = r.path.parent().ok_or("missing parent")?;
    private_dir(parent)?;
    write_new(&parent.join("owner"), r.marker.as_bytes())?;
    private_dir(&r.output)?;
    // No --force: Git refuses existing paths or refs. Filters/hooks were refused/disabled.
    git(
        &r.repo.path,
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            &r.branch,
            text_path(&r.path)?,
            &r.base,
        ],
        None,
    )?;
    if let Some(c) = candidate {
        c.apply(&r.path)?;
    }
    marker_check(&r)?;
    io(fs::File::open(parent).and_then(|f| f.sync_all()))?;
    Ok(MaterializedWorktree {
        record: r,
        snapshot: candidate.map(|c| c.digest.clone()),
    })
}
pub fn inspect_worktree(engine: &Engine, attempt: OpaqueId) -> IoResult<InspectedWorktree> {
    let r = current(engine, attempt)?;
    if !matches!(r.state.as_str(), "READY" | "BOUND") {
        return Err("unsealed worktree".into());
    }
    marker_check(&r)?;
    let captured = capture(&r.path, r.predecessor.unwrap_or(r.attempt), r.repository)?;
    if captured.head != r.base {
        return Err("worktree HEAD changed from frozen base".into());
    }
    match &r.snapshot {
        Some(d) if sha256_hex(&[&captured.bytes()]) != *d => {
            return Err("worktree snapshot changed before execution".into());
        }
        None if !captured.staged.is_empty()
            || !captured.unstaged.is_empty()
            || !captured.extras.is_empty() =>
        {
            return Err("worktree changed before execution".into());
        }
        _ => {}
    }
    Ok(InspectedWorktree { record: r })
}

/// Reopen inspection (P1-11): ownership checks, then two identical captures. Returns the
/// snapshot digest and HEAD. Writes nothing; not process-death evidence.
pub(crate) fn recapture(record: &WorktreeRecord) -> IoResult<(String, String)> {
    marker_check(record)?;
    let c = capture(&record.path, record.attempt, record.repository)?;
    if capture(&record.path, record.attempt, record.repository)? != c {
        return Err("worktree changed during capture".into());
    }
    Ok((sha256_hex(&[&c.bytes()]), c.head))
}

/// Passive restart observation. Does not mutate, release a claim, adopt or replay work.
pub fn observe_worktree(record: &WorktreeRecord) -> IoResult<String> {
    record.root.recheck()?;
    record.repo.recheck()?;
    marker_check(record)?;
    Ok(format!(
        "{}; retained; requires current-epoch recovery resolution",
        record.state
    ))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Extra {
    path: String,
    bytes: Vec<u8>,
    mode: u32,
    link: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSnapshot {
    attempt: OpaqueId,
    repository: RepositoryId,
    head: String,
    staged: Vec<u8>,
    unstaged: Vec<u8>,
    extras: Vec<Extra>,
    file: PathBuf,
    digest: String,
}
impl CandidateSnapshot {
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn path(&self) -> &Path {
        &self.file
    }
    fn bytes(&self) -> Vec<u8> {
        let mut b = vec![];
        for item in [
            id_to_text(self.attempt).into_bytes(),
            id_to_text(self.repository.0).into_bytes(),
            self.head.as_bytes().to_vec(),
            self.staged.clone(),
            self.unstaged.clone(),
        ]
        .into_iter()
        .chain(self.extras.iter().flat_map(|e| {
            [
                e.path.as_bytes().to_vec(),
                e.bytes.clone(),
                e.mode.to_be_bytes().to_vec(),
                vec![u8::from(e.link)],
            ]
        })) {
            b.extend_from_slice(&(item.len() as u64).to_be_bytes());
            b.extend(item);
        }
        b
    }
    fn verify(&self) -> IoResult<()> {
        if io(fs::symlink_metadata(&self.file))?
            .file_type()
            .is_symlink()
        {
            return Err("snapshot symlink".into());
        }
        let bytes = self.bytes();
        if io(fs::read(&self.file))? != bytes || sha256_hex(&[&bytes]) != self.digest {
            return Err("snapshot digest mismatch".into());
        }
        Ok(())
    }
    fn apply(&self, path: &Path) -> IoResult<()> {
        if !self.staged.is_empty() {
            git(
                path,
                &["apply", "--binary", "--index", "--whitespace=nowarn", "-"],
                Some(&self.staged),
            )?;
        }
        if !self.unstaged.is_empty() {
            git(
                path,
                &["apply", "--binary", "--whitespace=nowarn", "-"],
                Some(&self.unstaged),
            )?;
        }
        for e in &self.extras {
            let dest = contained(path, &e.path)?;
            if let Some(parent) = dest.parent() {
                io(fs::create_dir_all(parent))?;
            }
            if e.link {
                io(symlink(
                    std::str::from_utf8(&e.bytes).map_err(|_| "non UTF8 link")?,
                    &dest,
                ))?;
            } else {
                write_new(&dest, &e.bytes)?;
                io(fs::set_permissions(
                    &dest,
                    fs::Permissions::from_mode(e.mode),
                ))?;
            }
        }
        let captured = capture(path, self.attempt, self.repository)?;
        if captured.head != self.head
            || captured.staged != self.staged
            || captured.unstaged != self.unstaged
            || captured.extras != self.extras
        {
            return Err(
                "faithful continuation could not be established; preserved and blocked".into(),
            );
        }
        Ok(())
    }
}
fn contained(root: &Path, name: &str) -> IoResult<PathBuf> {
    let p = Path::new(name);
    if p.components().any(|c| !matches!(c, Component::Normal(_)))
        || name.is_empty()
        || p.components()
            .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
    {
        return Err("snapshot path traversal".into());
    }
    let mut dest = root.to_path_buf();
    for c in p.components() {
        dest.push(c);
        if let Ok(m) = fs::symlink_metadata(&dest)
            && m.file_type().is_symlink()
            && dest != root.join(p)
        {
            return Err("symlink parent escape".into());
        }
    }
    Ok(dest)
}
fn safe_link(name: &str, target: &Path) -> IoResult<()> {
    if target.is_absolute() {
        return Err("absolute symlink target blocked".into());
    }
    let mut depth = Path::new(name)
        .components()
        .count()
        .checked_sub(1)
        .ok_or("empty symlink path")?;
    for c in target.components() {
        match c {
            Component::ParentDir => {
                if depth == 0 {
                    return Err("symlink outside worktree blocked".into());
                }
                depth -= 1;
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => return Err("unsupported symlink".into()),
        }
    }
    Ok(())
}
fn capture(
    path: &Path,
    attempt: OpaqueId,
    repository: RepositoryId,
) -> IoResult<CandidateSnapshot> {
    if !git(
        path,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
        None,
    )?
    .is_empty()
    {
        return Err("ignored files omitted: snapshot blocked".into());
    }
    if !git(path, &["ls-files", "--unmerged", "-z"], None)?.is_empty() {
        return Err("unmerged index cannot be faithfully materialized".into());
    }
    let flags = git(path, &["ls-files", "-v", "-z"], None)?;
    if flags.split(|b| *b == 0).any(|s| {
        s.first()
            .is_some_and(|b| *b == b'S' || b.is_ascii_lowercase())
    }) {
        return Err("sparse/assume-unchanged index flags require explicit materialization".into());
    }
    let index = git(path, &["ls-files", "--stage", "-z"], None)?;
    if index.split(|b| *b == 0).any(|s| s.starts_with(b"160000 ")) {
        return Err("submodule contents require explicit materialization; blocked".into());
    }
    let names = git(
        path,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        None,
    )?;
    for n in names.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let n = std::str::from_utf8(n).map_err(|_| "non-UTF8 file name")?;
        let p = contained(path, n)?;
        if let Ok(m) = fs::symlink_metadata(&p)
            && !m.is_file()
            && !m.file_type().is_symlink()
        {
            return Err("non-file tracked content cannot be captured".into());
        }
        if let Ok(m) = fs::symlink_metadata(&p)
            && m.file_type().is_symlink()
        {
            let target = io(fs::read_link(&p))?;
            safe_link(n, &target)?;
        }
    }
    let mut extras = vec![];
    let mut seen = BTreeSet::new();
    for n in git(
        path,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        None,
    )?
    .split(|b| *b == 0)
    .filter(|n| !n.is_empty())
    {
        let name = std::str::from_utf8(n)
            .map_err(|_| "non-UTF8 file name")?
            .to_owned();
        if !seen.insert(name.clone()) {
            continue;
        }
        let p = contained(path, &name)?;
        let m = io(fs::symlink_metadata(&p))?;
        let link = m.file_type().is_symlink();
        if m.permissions().mode() & 0o7000 != 0 {
            return Err("special permission bits are not silently omitted".into());
        }
        if !link && !m.is_file() {
            return Err("special untracked file blocked".into());
        }
        let bytes = if link {
            io(fs::read_link(&p))?
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else {
            io(fs::read(&p))?
        };
        extras.push(Extra {
            path: name,
            bytes,
            mode: m.permissions().mode() & 0o777,
            link,
        });
    }
    let result = CandidateSnapshot {
        attempt,
        repository,
        head: line(git(path, &["rev-parse", "--verify", "HEAD"], None)?)?,
        staged: git(
            path,
            &[
                "diff",
                "--cached",
                "--binary",
                "--full-index",
                "--no-ext-diff",
                "--no-textconv",
                "--",
            ],
            None,
        )?,
        unstaged: git(
            path,
            &[
                "diff",
                "--binary",
                "--full-index",
                "--no-ext-diff",
                "--no-textconv",
                "--",
            ],
            None,
        )?,
        extras,
        file: PathBuf::new(),
        digest: String::new(),
    };
    if result.bytes().len() > 16 * 1024 * 1024 {
        return Err("snapshot size bound exceeded".into());
    }
    Ok(result)
}
/// Captures twice to detect cooperative concurrent edits. Call only after writer absence
/// is established by the lifecycle owner; this function is not process-death evidence.
pub fn collect_candidate(engine: &Engine, attempt: OpaqueId) -> IoResult<CandidateSnapshot> {
    let r = current_or_recovered(engine, attempt)?;
    if !matches!(r.state.as_str(), "READY" | "BOUND") {
        return Err("unsealed worktree".into());
    }
    marker_check(&r)?;
    let claims = engine
        .reader()
        .map_err(|e| e.to_string())?
        .read(|v| v.live_claims())
        .map_err(|e| e.to_string())?;
    if claims
        .iter()
        .any(|c| c.resource_key == format!("worktree:{}", id_to_text(attempt)))
    {
        return Err("candidate writer is still active or uncertain".into());
    }
    let mut c = capture(&r.path, attempt, r.repository)?;
    if capture(&r.path, attempt, r.repository)? != c {
        return Err("candidate changed during capture".into());
    }
    c.digest = sha256_hex(&[&c.bytes()]);
    c.file = r.output.join(format!("candidate-{}", c.digest));
    if c.file.exists() {
        c.verify()?;
    } else {
        write_new(&c.file, &c.bytes())?;
    }
    io(fs::File::open(&r.output).and_then(|f| f.sync_all()))?;
    Ok(c)
}

/// Repository-derived changes between the frozen base and a sealed candidate. Only
/// [`inspect_candidate_changes`] constructs it; validation never accepts a path list
/// supplied by a provider or validator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateChanges {
    attempt: OpaqueId,
    candidate: String,
    base: String,
    changes: Vec<ChangedPath>,
}
impl CandidateChanges {
    pub fn attempt(&self) -> OpaqueId {
        self.attempt
    }
    pub fn candidate(&self) -> &str {
        &self.candidate
    }
    pub fn base(&self) -> &str {
        &self.base
    }
    pub fn changes(&self) -> &[ChangedPath] {
        &self.changes
    }
}

/// Reads the retained worktree with the writer proven absent, requires it to reproduce
/// the sealed candidate digest before and after, and derives every changed path with
/// local Git (tracked: raw diff against the frozen base commit, no rename folding;
/// untracked: exclude-standard listing). Ignored, unmerged, submodule, special or
/// non-UTF-8 entries fail closed. Nothing in the worktree is modified.
pub fn inspect_candidate_changes(
    engine: &Engine,
    attempt: OpaqueId,
    candidate: &str,
) -> IoResult<CandidateChanges> {
    let r = current_or_recovered(engine, attempt)?;
    if !matches!(r.state.as_str(), "READY" | "BOUND") {
        return Err("unsealed worktree".into());
    }
    marker_check(&r)?;
    let claims = engine
        .reader()
        .map_err(|e| e.to_string())?
        .read(|v| v.live_claims())
        .map_err(|e| e.to_string())?;
    if claims
        .iter()
        .any(|c| c.resource_key == format!("worktree:{}", id_to_text(attempt)))
    {
        return Err("candidate writer is still active or uncertain".into());
    }
    let reproduces = |when: &str| -> IoResult<()> {
        let c = capture(&r.path, attempt, r.repository)?;
        if sha256_hex(&[&c.bytes()]) != candidate {
            return Err(format!(
                "worktree does not reproduce the sealed candidate {when}"
            ));
        }
        Ok(())
    };
    reproduces("before inspection")?;
    let raw = git(
        &r.path,
        &[
            "diff",
            "--raw",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=none",
            "--no-abbrev",
            &r.base,
            "--",
        ],
        None,
    )?;
    let mut changes = parse_raw_diff(&raw)?;
    for n in git(
        &r.path,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        None,
    )?
    .split(|b| *b == 0)
    .filter(|n| !n.is_empty())
    {
        let name = std::str::from_utf8(n).map_err(|_| "non-UTF8 file name")?;
        ScopePath::parse(name)?;
        let m = io(fs::symlink_metadata(contained(&r.path, name)?))?;
        let new_mode = if m.file_type().is_symlink() {
            0o120000
        } else if !m.is_file() {
            return Err("special untracked file blocked".into());
        } else if m.permissions().mode() & 0o111 != 0 {
            0o100755
        } else {
            0o100644
        };
        changes.push(ChangedPath {
            path: name.to_owned(),
            kind: ChangeKind::Added,
            old_mode: 0,
            new_mode,
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    if changes.windows(2).any(|w| w[0].path == w[1].path) {
        return Err("ambiguous duplicate changed path".into());
    }
    reproduces("after inspection")?;
    Ok(CandidateChanges {
        attempt,
        candidate: candidate.to_owned(),
        base: r.base.clone(),
        changes,
    })
}

impl Tx<'_> {
    /// Preferred production admission entry: reserves the full scheduler envelope and
    /// private writers together, using freshly inspected pre-provisioned bindings.
    pub fn schedule_owned_worktrees(
        &mut self,
        config: u64,
        inspected: Vec<InspectedWorktree>,
    ) -> Result<Vec<crate::AdmissionDecision>, TxError> {
        let ids: Vec<_> = inspected.iter().map(|p| p.record.attempt).collect();
        let bindings = inspected
            .iter()
            .map(|p| (p.record.attempt, p.record.repository))
            .collect();
        let decisions = self.schedule_with_bindings(config, &ids, Some(&bindings))?;
        for proof in inspected {
            if decisions
                .iter()
                .any(|d| d.attempt == Some(proof.record.attempt))
            {
                self.bind_worktree_execution(proof)?;
            }
        }
        Ok(decisions)
    }
}

impl CandidateSnapshot {
    /// Reopens the exact durable packet after interruption. No source worktree edits
    /// are required; a changed/partial packet or unexpected field is rejected.
    pub fn reopen(file: &Path, expected_digest: &str) -> IoResult<Self> {
        if io(fs::symlink_metadata(file))?.file_type().is_symlink() {
            return Err("snapshot symlink".into());
        }
        let file = canonical(file)?;
        let bytes = io(fs::read(&file))?;
        if bytes.len() > 16 * 1024 * 1024 || sha256_hex(&[&bytes]) != expected_digest {
            return Err("snapshot digest/size mismatch".into());
        }
        let mut rest = bytes.as_slice();
        fn field(rest: &mut &[u8]) -> IoResult<Vec<u8>> {
            if rest.len() < 8 {
                return Err("truncated snapshot length".into());
            }
            let len = u64::from_be_bytes(rest[..8].try_into().map_err(|_| "length")?) as usize;
            *rest = &rest[8..];
            if len > rest.len() {
                return Err("truncated snapshot field".into());
            }
            let v = rest[..len].to_vec();
            *rest = &rest[len..];
            Ok(v)
        }
        fn string(bytes: Vec<u8>) -> IoResult<String> {
            String::from_utf8(bytes).map_err(|_| "non-UTF8 snapshot metadata".into())
        }
        let parse = |s: String| {
            u128::from_str_radix(&s, 16)
                .map(OpaqueId)
                .map_err(|_| "bad snapshot id".to_owned())
        };
        let attempt = parse(string(field(&mut rest)?)?)?;
        let repository = RepositoryId(parse(string(field(&mut rest)?)?)?);
        let head = string(field(&mut rest)?)?;
        if !oid(&head) {
            return Err("bad candidate commit".into());
        }
        let staged = field(&mut rest)?;
        let unstaged = field(&mut rest)?;
        let mut extras = vec![];
        let mut seen = BTreeSet::new();
        while !rest.is_empty() {
            let path = string(field(&mut rest)?)?;
            contained(Path::new("/"), &path)?;
            if !seen.insert(path.clone()) {
                return Err("duplicate snapshot path".into());
            }
            let bytes = field(&mut rest)?;
            let mode = u32::from_be_bytes(field(&mut rest)?.try_into().map_err(|_| "bad mode")?);
            let flag = field(&mut rest)?;
            if flag != [0] && flag != [1] {
                return Err("bad symlink flag".into());
            }
            if mode & !0o777 != 0 {
                return Err("unsupported snapshot permission bits".into());
            }
            if flag == [1] {
                safe_link(
                    &path,
                    Path::new(std::str::from_utf8(&bytes).map_err(|_| "non-UTF8 symlink")?),
                )?;
            }
            extras.push(Extra {
                path,
                bytes,
                mode,
                link: flag == [1],
            });
        }
        let result = Self {
            attempt,
            repository,
            head,
            staged,
            unstaged,
            extras,
            file,
            digest: expected_digest.into(),
        };
        result.verify()?;
        Ok(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateReference {
    pub attempt: OpaqueId,
    pub digest: String,
    pub head: String,
    pub path: PathBuf,
}
impl ReadView<'_> {
    pub fn candidate_reference(
        &self,
        attempt: OpaqueId,
        digest: &str,
    ) -> Result<Option<CandidateReference>, StorageError> {
        Ok(self.conn.query_optional(
            "SELECT head,path FROM worktree_candidates WHERE attempt=?1 AND digest=?2",
            &[Value::Text(&id_to_text(attempt)), Value::Text(digest)],
            |r| {
                Ok(CandidateReference {
                    attempt,
                    digest: digest.into(),
                    head: r.text(0)?,
                    path: PathBuf::from(r.text(1)?),
                })
            },
        )?)
    }
}

/// Upper bound of the diff text served for review; longer diffs are cut and marked.
pub const MAX_REVIEW_DIFF_BYTES: usize = 1024 * 1024;

/// P2-03: materializes the approved base plus the sealed candidate in a new, private,
/// Jojo-owned directory `dest` (which must not exist) and returns the checked-out tree.
///
/// The candidate comes only from the durable packet, reopened against its recorded digest,
/// so a packet changed after sealing is refused. The tree is an independent clone (objects
/// copied, no hooks, no templates, no shared files) of the registered repository, checked
/// out at the candidate's commit with the packet applied; the result must capture to
/// exactly the packet again. Neither the source repository nor the developer worktree is
/// written to, and the developer worktree is never the validation directory.
pub fn materialize_validation_tree(
    engine: &Engine,
    attempt: OpaqueId,
    candidate: &str,
    dest: &Path,
) -> IoResult<PathBuf> {
    let r = current_or_recovered(engine, attempt)?;
    let reference = engine
        .reader()
        .map_err(|e| e.to_string())?
        .read(|v| v.candidate_reference(attempt, candidate))
        .map_err(|e| e.to_string())?
        .ok_or("candidate is not durably recorded")?;
    let packet = CandidateSnapshot::reopen(&reference.path, candidate)
        .map_err(|e| format!("sealed candidate packet rejected: {e}"))?;
    if packet.attempt != attempt || packet.repository != r.repository || !oid(&r.base) {
        return Err("sealed candidate packet belongs to another attempt".into());
    }
    let parent = dest.parent().ok_or("validation directory has no parent")?;
    if canonical(parent)? != parent || dest.starts_with(&r.path) || dest.starts_with(&r.repo.path) {
        return Err("validation directory must be a separate owned path".into());
    }
    private_dir(dest)?;
    git(
        dest,
        &[
            "clone",
            "--no-checkout",
            "--no-hardlinks",
            "--template=",
            "--quiet",
            "--",
            text_path(&r.repo.path)?,
            "tree",
        ],
        None,
    )?;
    let tree = dest.join("tree");
    for commit in [&r.base, &packet.head] {
        let found = line(git(
            &tree,
            &["rev-parse", "--verify", &format!("{commit}^{{commit}}")],
            None,
        )?)?;
        if found != **commit {
            return Err("approved base or candidate commit is not an exact commit".into());
        }
    }
    git(
        &tree,
        &["checkout", "--detach", "--quiet", &packet.head],
        None,
    )?;
    packet.apply(&tree)?;
    Ok(tree)
}

/// P2-03: bounded unified diff of the frozen base against the sealed candidate, for human
/// review only. Derived by local Git from the retained worktree, which must reproduce the
/// sealed digest before and after; untracked files are listed with their text. Returns
/// the text and whether it is complete.
pub fn candidate_diff(
    engine: &Engine,
    attempt: OpaqueId,
    candidate: &str,
) -> IoResult<(String, bool)> {
    let changes = inspect_candidate_changes(engine, attempt, candidate)?;
    let r = current_or_recovered(engine, attempt)?;
    let mut bytes = git(
        &r.path,
        &[
            "diff",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--ignore-submodules=none",
            &r.base,
            "--",
        ],
        None,
    )?;
    let tracked: BTreeSet<Vec<u8>> = git(&r.path, &["ls-files", "--cached", "-z"], None)?
        .split(|b| *b == 0)
        .map(<[u8]>::to_vec)
        .collect();
    for c in changes.changes() {
        if c.kind != ChangeKind::Added || tracked.contains(c.path.as_bytes()) {
            continue;
        }
        if bytes.len() > MAX_REVIEW_DIFF_BYTES {
            break;
        }
        let p = contained(&r.path, &c.path)?;
        bytes.extend(format!("untracked new file {}\n", c.path).into_bytes());
        match fs::symlink_metadata(&p) {
            Ok(m) if m.is_file() && m.len() <= MAX_REVIEW_DIFF_BYTES as u64 => {
                match String::from_utf8(io(fs::read(&p))?) {
                    Ok(text) => text
                        .lines()
                        .for_each(|l| bytes.extend(format!("+{l}\n").into_bytes())),
                    Err(_) => bytes.extend(b"(binary content not shown)\n"),
                }
            }
            _ => bytes.extend(b"(content not shown)\n"),
        }
    }
    if inspect_candidate_changes(engine, attempt, candidate)? != changes {
        return Err("candidate changed during diff".into());
    }
    let complete = bytes.len() <= MAX_REVIEW_DIFF_BYTES;
    bytes.truncate(MAX_REVIEW_DIFF_BYTES);
    Ok((String::from_utf8_lossy(&bytes).into_owned(), complete))
}

/// P2-03: the sealed candidate is still what was sealed: the durable packet has its
/// recorded digest and the retained worktree reproduces it. Read-only.
pub fn verify_sealed_candidate(
    engine: &Engine,
    attempt: OpaqueId,
    candidate: &str,
) -> IoResult<CandidateChanges> {
    let reference = engine
        .reader()
        .map_err(|e| e.to_string())?
        .read(|v| v.candidate_reference(attempt, candidate))
        .map_err(|e| e.to_string())?
        .ok_or("candidate is not durably recorded")?;
    CandidateSnapshot::reopen(&reference.path, candidate)
        .map_err(|e| format!("sealed candidate packet rejected: {e}"))?;
    inspect_candidate_changes(engine, attempt, candidate)
}
