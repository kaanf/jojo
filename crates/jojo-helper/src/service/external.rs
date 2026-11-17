//! M5: push and draft pull request of a finished plan branch (protocol 1.10).
//!
//! A request performs nothing. An authorization is decided and the action issued in one
//! transaction (`ISSUED_UNKNOWN`); only then does one worker thread run the push or the
//! pull request, once, and record what it saw. A replayed command answers from its
//! receipt and starts nothing; a restarted helper never runs an issued action again. An
//! inspection reads the remote and records what it shows; it never sends anything.
//! The remote is reached with git's own credential helper and gh's own sign-in, in the
//! user's HOME; the helper holds no token.
use super::commands::rejected;
use super::*;
use jojo_domain::identity::RepositoryId;
use jojo_storage::remote::{self, Outcome, RemoteEnv};
use std::collections::HashSet;
use std::sync::Mutex;

/// How often the sign-in state of git and gh is read again at most.
const AUTH_AGE: Duration = Duration::from_secs(60);

/// How this helper reaches a remote. Fixed at start by its launcher.
#[derive(Clone)]
pub struct RemoteConfig {
    /// Finds `gh` on the login shell's PATH (tests: a given PATH).
    pub discovery: Discovery,
    /// The user's HOME: git's configuration and credential helper, gh's sign-in.
    pub home: PathBuf,
    auth: Arc<Mutex<AuthCache>>,
}

#[derive(Default)]
struct AuthCache {
    checked: Option<Instant>,
    checking: bool,
    git: Option<(&'static str, String)>,
    gh: Option<(&'static str, String)>,
}

impl RemoteConfig {
    pub fn new(discovery: Discovery, home: PathBuf) -> Self {
        Self {
            discovery,
            home,
            auth: Arc::default(),
        }
    }

    /// `gh` as discovery found it; never waits.
    fn gh(&self) -> Result<PathBuf, String> {
        match self.discovery.tool("gh") {
            None => {
                Err("GhChecking: gh is being looked up on the login shell's PATH; ask again".into())
            }
            Some(Err(why)) => Err(format!(
                "GhMissing: {why}; install the GitHub CLI and sign in with gh auth login"
            )),
            Some(Ok(path)) => Ok(path),
        }
    }

    fn env(&self, gh: Option<&Path>) -> RemoteEnv {
        RemoteEnv::new(&self.home, gh)
    }

    /// The sign-in state of git and gh as last read; reading again happens on a
    /// background thread when it is older than a minute.
    fn auth_json(&self) -> Json {
        let mut cache = self.auth.lock().unwrap_or_else(|p| p.into_inner());
        let stale = cache.checked.is_none_or(|t| t.elapsed() >= AUTH_AGE);
        if stale && !cache.checking {
            cache.checking = true;
            let config = self.clone();
            let spawned = std::thread::Builder::new()
                .name("jojo-remote-auth".into())
                .spawn(move || config.check_auth());
            if spawned.is_err() {
                cache.checking = false;
            }
        }
        let entry = |v: &Option<(&'static str, String)>| {
            let (state, detail) = v.clone().unwrap_or(("Checking", "being read".into()));
            object([("state", text(state)), ("detail", text(detail))])
        };
        object([("git", entry(&cache.git)), ("gh", entry(&cache.gh))])
    }

    fn check_auth(&self) {
        let mut found = None;
        for _ in 0..100 {
            if let Some(f) = self.discovery.tool("gh") {
                found = Some(f);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let gh = match found {
            None => ("Checking", "gh is still being looked up".to_owned()),
            Some(Err(why)) => ("Missing", format!("gh was not found: {why}")),
            Some(Ok(path)) => match remote::gh_signed_in(&self.env(Some(&path)), &path, &self.home)
            {
                Ok(line) => (
                    "Ready",
                    if line.is_empty() {
                        "signed in".into()
                    } else {
                        line
                    },
                ),
                Err(why) => ("Missing", why),
            },
        };
        let helpers = remote::credential_helpers(&self.env(None), &self.home);
        let git = if helpers.is_empty() {
            (
                "Missing",
                "git has no credential helper; an HTTPS push would ask for a password, which \
                 Jojo never answers"
                    .to_owned(),
            )
        } else {
            (
                "Ready",
                format!("credential helper: {}", helpers.join(", ")),
            )
        };
        let mut cache = self.auth.lock().unwrap_or_else(|p| p.into_inner());
        cache.git = Some(git);
        cache.gh = Some(gh);
        cache.checked = Some(Instant::now());
        cache.checking = false;
    }
}

/// M5 test faults, injected after an action is issued.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalFault {
    /// The action reaches the remote, then its answer is lost: what is recorded is an
    /// unknown outcome seen by `fake-remote:lost-outcome`.
    LoseOutcome,
    /// The helper stops after issuing and before running anything: nothing runs and
    /// nothing is recorded, as after a crash.
    StopBeforeDispatch,
}

fn actor(origin: &HumanOrigin) -> String {
    match origin {
        HumanOrigin::TrustedUi { actor } | HumanOrigin::FakeHumanFixture { actor } => actor.clone(),
    }
}

/// Records one observation under a runtime command identity.
fn record(
    engine: &Engine,
    ids: &RuntimeIds,
    key: &str,
    observation: ExternalObservation,
) -> Result<(), String> {
    let id = ids.next_id().map_err(|_| "IDs exhausted")?;
    let request = CommandRequest::new(
        CommandId(id),
        engine.epoch(),
        "ObserveExternal",
        key.to_owned(),
    );
    let key = key.to_owned();
    engine
        .execute(request, move |tx| tx.observe_external(&key, &observation))
        .map(drop)
        .map_err(|e| format!("{e:?}"))
}

fn observed(observer: &str, outcome: Outcome) -> ExternalObservation {
    let (status, detail, number, url) = match outcome {
        Outcome::Committed {
            detail,
            number,
            url,
        } => (ExternalStatus::Committed, detail, number, url),
        Outcome::Rejected(detail) => (ExternalStatus::Rejected, detail, None, None),
        Outcome::Unknown(detail) => (ExternalStatus::Unknown, detail, None, None),
    };
    ExternalObservation {
        observer: observer.into(),
        status,
        detail,
        number,
        url,
    }
}

impl Service {
    fn remote_config(&self) -> Result<RemoteConfig, String> {
        self.remote
            .clone()
            .ok_or_else(|| "RemoteNotConfigured: this helper reaches no remote".into())
    }

    fn read<T>(
        &self,
        f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>,
    ) -> Result<T, String> {
        self.engine
            .reader()
            .and_then(|mut r| r.read(f))
            .map_err(|e| e.to_string())
    }

    fn checkout(&self, repository: RepositoryId) -> Result<PathBuf, String> {
        self.read(|v| v.repository_path(repository))?
            .ok_or_else(|| "ExternalRefused: the plan's repository is not registered".into())
    }

    fn in_flight(&self) -> HashSet<String> {
        self.in_flight
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Protocol 1.10: a push or a draft pull request of a finished plan, as a request.
    pub(super) fn request_external(
        &mut self,
        request: CommandRequest,
        p: &Json,
    ) -> Result<Json, String> {
        let protocol::RequestExternalCommand { plan, kind } =
            protocol::RequestExternalCommand::parse(p)?;
        let config = self.remote_config()?;
        let run = self
            .read(|v| v.plan_run(plan))?
            .ok_or("ExternalRefused: this plan was never started")?;
        let path = self.checkout(run.repository)?;
        let env = config.env(config.gh().ok().as_deref());
        let url = remote::remote_url(&env, &path, REMOTE)
            .map_err(|why| format!("ExternalRefused: {why}"))?;
        let (kind, base) = match kind {
            protocol::ExternalKindName::Push => (ExternalKind::Push, None),
            protocol::ExternalKindName::PullRequest => {
                config.gh()?;
                let base = remote::default_branch(&env, &path, &url)
                    .map_err(|why| format!("RemoteUnreachable: {why}"))?;
                (ExternalKind::PullRequest, Some(base))
            }
        };
        self.engine
            .execute(request, move |tx| {
                tx.request_external(plan, kind, &url, base.as_deref())
            })
            .map(receipt)
            .map_err(rejected)
    }

    /// Protocol 1.10: the human's decision on one request. An authorized action is issued
    /// in the decision's transaction and then performed once, on a worker thread.
    pub(super) fn authorize_external(
        &mut self,
        request: CommandRequest,
        p: &Json,
        origin: HumanOrigin,
    ) -> Result<Json, String> {
        let protocol::AuthorizeExternalCommand {
            action,
            digest,
            authorize,
        } = protocol::AuthorizeExternalCommand::parse(p)?;
        if authorize {
            let config = self.remote_config()?;
            let known = self
                .read(|v| v.external_action(&action))?
                .ok_or("UnknownExternalAction: no such request")?;
            if known.kind == ExternalKind::PullRequest {
                config.gh()?;
            }
        }
        let (key, who) = (action.clone(), actor(&origin));
        let r = self
            .engine
            .execute(request, move |tx| {
                tx.decide_external(&key, &digest, authorize, &who)
            })
            .map_err(rejected)?;
        let replayed = r.replayed;
        let Json::Object(mut fields) = receipt(r) else {
            unreachable!("receipt is an object")
        };
        if !authorize || replayed {
            return Ok(Json::Object(fields));
        }
        let started = self.dispatch_external(&action);
        fields.push((
            "dispatch".into(),
            text(match started {
                Ok(()) => "started".to_owned(),
                Err(why) => why,
            }),
        ));
        Ok(Json::Object(fields))
    }

    /// Runs one issued action once and records what it saw. If it cannot even start, that
    /// is recorded as not performed: nothing left the machine.
    fn dispatch_external(&mut self, key: &str) -> Result<(), String> {
        let action = self
            .read(|v| v.external_action(key))?
            .ok_or("UnknownExternalAction: no such action")?;
        let config = self.remote_config()?;
        let path = self.checkout(action.repository)?;
        let (engine, ids) = (self.engine.clone(), self.ids.clone());
        let gh = match (action.kind, config.gh()) {
            (_, Ok(gh)) => Some(gh),
            (ExternalKind::Push, Err(_)) => None,
            (ExternalKind::PullRequest, Err(why)) => {
                let observation = observed("gh:pr-create", Outcome::Rejected(why.clone()));
                record(&engine, &ids, key, observation)?;
                return Err(why);
            }
        };
        let env = config.env(gh.as_deref());
        let fault = self.external_fault;
        let in_flight = self.in_flight.clone();
        in_flight
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_owned());
        let owned = key.to_owned();
        let started = self.workers.external(move || {
            let result = (|| {
                if fault == Some(ExternalFault::StopBeforeDispatch) {
                    return Ok(());
                }
                let (observer, outcome) = match action.kind {
                    ExternalKind::Push => (
                        "git:push",
                        remote::push(&env, &path, &action.url, &action.commit, &action.branch),
                    ),
                    ExternalKind::PullRequest => (
                        "gh:pr-create",
                        remote::create_draft(
                            &env,
                            gh.as_deref().unwrap_or(Path::new("gh")),
                            &path,
                            &action.url,
                            action.base.as_deref().unwrap_or(""),
                            &action.branch,
                            action.title.as_deref().unwrap_or(""),
                            action.body.as_deref().unwrap_or(""),
                        ),
                    ),
                };
                let observation = if fault == Some(ExternalFault::LoseOutcome) {
                    observed(
                        "fake-remote:lost-outcome",
                        Outcome::Unknown("the answer was lost on the way back (test fault)".into()),
                    )
                } else {
                    observed(observer, outcome)
                };
                record(&engine, &ids, &owned, observation)
            })();
            in_flight
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&owned);
            result
        });
        if let Err(why) = started {
            self.in_flight
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(key);
            let observer = match action.kind {
                ExternalKind::Push => "git:not-started",
                ExternalKind::PullRequest => "gh:not-started",
            };
            let observation = observed(observer, Outcome::Rejected(format!("not started: {why}")));
            record(&self.engine, &self.ids, key, observation)?;
            return Err(format!("not started: {why}"));
        }
        Ok(())
    }

    /// Protocol 1.10: reads the remote for an action whose outcome is unknown and records
    /// what it shows. Sends nothing.
    pub(super) fn inspect_external(
        &mut self,
        request: CommandRequest,
        p: &Json,
    ) -> Result<Json, String> {
        let protocol::ExternalCommand { action } = protocol::ExternalCommand::parse(p)?;
        if self.in_flight().contains(&action) {
            return Err("ExternalInFlight: the action is still running".into());
        }
        let config = self.remote_config()?;
        let known = self
            .read(|v| v.external_action(&action))?
            .ok_or("UnknownExternalAction: no such action")?;
        let gh = match known.kind {
            ExternalKind::PullRequest => Some(config.gh()?),
            ExternalKind::Push => config.gh().ok(),
        };
        let path = self.checkout(known.repository)?;
        let key = action.clone();
        let r = self
            .engine
            .execute(request, move |tx| tx.begin_external_inspection(&key))
            .map_err(rejected)?;
        let Json::Object(mut fields) = receipt(r) else {
            unreachable!("receipt is an object")
        };
        let env = config.env(gh.as_deref());
        let (engine, ids) = (self.engine.clone(), self.ids.clone());
        let started = self.workers.external(move || {
            let observation = match known.kind {
                ExternalKind::Push => {
                    let seen = remote::remote_branch(&env, &path, &known.url, &known.branch);
                    let (status, detail) = match seen {
                        Ok(Some(c)) if c == known.commit => (
                            ExternalStatus::Committed,
                            format!(
                                "the remote branch is at the pushed commit {}",
                                &c[..12.min(c.len())]
                            ),
                        ),
                        Ok(Some(c)) => (
                            ExternalStatus::Unknown,
                            format!("the remote branch is at {c}, not at the pushed commit"),
                        ),
                        Ok(None) => (
                            ExternalStatus::Unknown,
                            format!("the remote has no branch {}", known.branch),
                        ),
                        Err(why) => (ExternalStatus::Unknown, why),
                    };
                    ExternalObservation {
                        observer: "git:ls-remote".into(),
                        status,
                        detail,
                        number: None,
                        url: None,
                    }
                }
                ExternalKind::PullRequest => {
                    let gh = gh.unwrap_or_default();
                    match remote::find_pull_request(&env, &gh, &path, &known.url, &known.branch) {
                        Ok(Some(pr)) => ExternalObservation {
                            observer: "gh:pr-list".into(),
                            status: ExternalStatus::Committed,
                            detail: format!(
                                "pull request #{} found{}",
                                pr.number,
                                if pr.draft {
                                    " (draft)"
                                } else {
                                    " (not a draft)"
                                }
                            ),
                            number: Some(pr.number),
                            url: Some(pr.url),
                        },
                        Ok(None) => ExternalObservation {
                            observer: "gh:pr-list".into(),
                            status: ExternalStatus::Unknown,
                            detail: "no pull request has the plan branch as its head".into(),
                            number: None,
                            url: None,
                        },
                        Err(why) => ExternalObservation {
                            observer: "gh:pr-list".into(),
                            status: ExternalStatus::Unknown,
                            detail: why,
                            number: None,
                            url: None,
                        },
                    }
                }
            };
            record(&engine, &ids, &action, observation)
        });
        fields.push((
            "inspection".into(),
            text(match started {
                Ok(()) => "started".to_owned(),
                Err(why) => format!("not started: {why}"),
            }),
        ));
        Ok(Json::Object(fields))
    }

    /// Protocol 1.10: the human marks an inspected action with an unknown outcome as not
    /// performed.
    pub(super) fn resolve_external(
        &mut self,
        request: CommandRequest,
        p: &Json,
        origin: HumanOrigin,
    ) -> Result<Json, String> {
        let protocol::ExternalCommand { action } = protocol::ExternalCommand::parse(p)?;
        if self.in_flight().contains(&action) {
            return Err("ExternalInFlight: the action is still running".into());
        }
        let who = actor(&origin);
        self.engine
            .execute(request, move |tx| tx.resolve_external(&action, &who))
            .map(receipt)
            .map_err(rejected)
    }

    /// Protocol 1.10: the runs and actions of one plan or of all, with the sign-in state
    /// of git and gh.
    pub(super) fn external_actions(&mut self, e: &Envelope) -> Result<Json, String> {
        if !RequestKind::ExternalActions.shaped(&e.payload) {
            return Err("InvalidPayload: unexpected or missing fields".into());
        }
        let q = protocol::ExternalActionsQuery::parse(&e.payload)?;
        let in_flight = self.in_flight();
        let answer = self.read(|v| v.ipc_external_actions(q.plan, &|k| in_flight.contains(k)))?;
        let auth = match &self.remote {
            Some(config) => config.auth_json(),
            None => {
                let none = || {
                    object([
                        ("state", text("NotConfigured")),
                        ("detail", text("this helper reaches no remote")),
                    ])
                };
                object([("git", none()), ("gh", none())])
            }
        };
        let Json::Object(mut fields) = answer else {
            unreachable!("an object")
        };
        fields.push(("auth".into(), auth));
        Ok(Json::Object(fields))
    }
}
