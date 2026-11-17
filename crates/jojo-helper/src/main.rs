use std::path::PathBuf;
use std::process::ExitCode;

use jojo_helper::{Invocation, parse_invocation, run_engine_probe};

fn main() -> ExitCode {
    // The only environment lookup: caller-owned HOME for macOS path defaults.
    // No credential or provider config. D3: a serving helper asks the login shell for
    // PATH with this HOME as its only variable, and runs a found CLI with `--version`.
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match parse_invocation(std::env::args_os().skip(1), home.clone()) {
        Ok(Invocation::Help) => {
            println!(
                "jojo-helper --check-config [--data-root PATH] [--worktree-root PATH]\n  validates paths and exits; creates nothing.\njojo-helper --engine-probe [--data-root PATH] [--worktree-root PATH]\n  owns the database (new engine epoch, admission closed) until stdin closes, then\n  records a clean shutdown. Status on stderr; exit 3 if another helper owns it.\n--serve starts the P1-13 bounded stdio protocol; fake-provider foundation only.\n  --serve [--disposable-root PATH [--codex-executable PATH --codex-version TEXT\n  --codex-sha256 HEX --codex-home PATH --codex-auth native-subscription]] enables the\n  P2-02 slice commands. Nothing is launched without a human Start.\n  D3: without --codex-executable (or --claude-executable) the CLI is found on the PATH\n  of the login shell (--login-shell PATH, default /bin/zsh) before each run; the path\n  alone has its version and digest learned; path, version and digest together pin it."
            );
            ExitCode::SUCCESS
        }
        Ok(Invocation::Version) => {
            println!(
                "jojo-helper {} (P1-02 foundation; system SQLite {})",
                env!("CARGO_PKG_VERSION"),
                jojo_storage::sqlite_version_number()
            );
            ExitCode::SUCCESS
        }
        Ok(Invocation::CheckConfig(roots)) => {
            eprintln!("P1-01 configuration only; no directories created.");
            eprintln!("data root: {}", roots.data().display());
            eprintln!("worktree root: {}", roots.worktrees().display());
            ExitCode::SUCCESS
        }
        Ok(invocation @ (Invocation::Serve(_) | Invocation::ServeSlice(..))) => {
            let (roots, config, claude, discovered) = match invocation {
                Invocation::ServeSlice(roots, options) => {
                    let claude = options.claude.clone();
                    // D3: the login shell gets the helper's own HOME and nothing else.
                    let found =
                        jojo_helper::service::Discovered::from_options(&options, home.as_deref());
                    let discovered = match found {
                        Ok(discovered) => discovered,
                        Err(why) => {
                            eprintln!("jojo-helper: executable discovery: {why}");
                            return ExitCode::from(2);
                        }
                    };
                    let slice = jojo_helper::service::SliceConfig::new(&roots, options);
                    (
                        roots,
                        jojo_helper::service::ServiceConfig::slice(slice),
                        claude,
                        discovered,
                    )
                }
                Invocation::Serve(roots) => (roots, Default::default(), None, None),
                _ => unreachable!("matched serve invocations only"),
            };
            // P2-04.1: every process group this helper starts is guarded by a second
            // instance of this same executable. Without it nothing may be launched.
            let Ok(guard) = std::env::current_exe() else {
                eprintln!("jojo-helper: cannot locate its own executable for the agent guard");
                return ExitCode::from(4);
            };
            let engine = jojo_storage::prepare_data_root(roots.data()).and_then(|db| {
                jojo_storage::Engine::start(jojo_storage::EngineConfig::new(
                    db,
                    std::sync::Arc::new(jojo_helper::SystemClock::new()),
                ))
            });
            match engine {
                Ok(engine) => match jojo_helper::service::Service::with_config(
                    std::sync::Arc::new(engine),
                    config,
                ) {
                    Ok(mut service) => {
                        service.guard = Some(guard);
                        service.claude =
                            claude.and_then(|c| jojo_helper::service::claude_config(*c));
                        service.discovered = discovered;
                        service.begin_executable_checks();
                        ExitCode::from(jojo_helper::service::serve(
                            service,
                            std::io::stdin(),
                            std::io::stdout(),
                        ))
                    }
                    Err(e) => {
                        eprintln!("jojo-helper: {e}");
                        ExitCode::from(4)
                    }
                },
                Err(jojo_storage::StorageError::OwnershipHeld { .. }) => ExitCode::from(3),
                Err(e) => {
                    eprintln!("jojo-helper: {e}");
                    ExitCode::from(4)
                }
            }
        }
        Ok(Invocation::AgentGuard { leader, inspector }) => ExitCode::from(
            jojo_storage::run_agent_guard(leader, &inspector, std::io::stdin()),
        ),
        Ok(Invocation::EngineProbe(roots)) => ExitCode::from(run_engine_probe(
            &roots,
            std::io::stdin().lock(),
            std::io::stderr(),
        )),
        Err(error) => {
            eprintln!("jojo-helper: {error:?}; use --help");
            ExitCode::from(2)
        }
    }
}
