import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("boundaries", ROOT / "scripts/check_p1_01_boundaries.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class BoundaryRegressionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="jojo-boundary-test-")
        self.root = Path(self.temp.name)
        for name in ["Cargo.toml", "Cargo.lock"]:
            if (ROOT / name).exists():
                shutil.copy2(ROOT / name, self.root / name)
        for name in ["crates", ".cargo", "macos/JojoApp"]:
            shutil.copytree(ROOT / name, self.root / name, ignore=shutil.ignore_patterns(".build", ".swiftpm"))
        self.addCleanup(self.temp.cleanup)

    def mutate(self, relative, suffix):
        with (self.root / relative).open("a") as out:
            out.write(suffix)
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_current_source_obeys_boundary(self):
        self.assertEqual(CHECK.boundary_errors(self.root), [])

    def test_provider_network_dependency_is_rejected(self):
        self.mutate("crates/jojo-helper/Cargo.toml", '\n[dev-dependencies]\nreqwest = "0.12"\n')

    def test_external_production_dependency_is_rejected(self):
        p = self.root / "crates/jojo-domain/Cargo.toml"
        p.write_text(p.read_text().replace('[dependencies]', '[dependencies]\nreqwest = "0.12"'))
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_changed_registry_checksum_is_rejected(self):
        p = self.root / "Cargo.lock"
        p.write_text(p.read_text().replace('checksum = "', 'checksum = "0', 1))
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_unapproved_dependency_feature_is_rejected(self):
        p = self.root / "crates/jojo-storage/Cargo.toml"
        p.write_text(p.read_text().replace('["extra_check"]', '["extra_check", "bundled"]'))
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_unsafe_in_sqlite_adapter_is_rejected(self):
        self.mutate("crates/jojo-storage/src/sqlite.rs", '\nfn forbidden() { unsafe {} }\n')

    def test_child_process_launch_is_rejected(self):
        self.mutate("crates/jojo-helper/src/main.rs", '\nfn forbidden() { std::process::Command::new("claude"); }\n')

    def test_network_api_is_rejected(self):
        self.mutate("crates/jojo-helper/src/main.rs", '\nuse std::net::TcpStream;\n')

    def test_credential_environment_read_is_rejected(self):
        self.mutate("crates/jojo-helper/src/main.rs", '\nfn forbidden() { std::env::var_os("OPENAI_API_KEY"); }\n')

    def test_ambient_domain_time_is_rejected(self):
        self.mutate("crates/jojo-domain/src/time.rs", '\nfn forbidden() { std::time::SystemTime::now(); }\n')

    def test_compile_time_credential_read_is_rejected(self):
        self.mutate("crates/jojo-helper/src/main.rs", '\nconst FORBIDDEN: Option<&str> = option_env!("OPENAI_API_KEY");\n')

    def test_aliased_filesystem_access_is_rejected(self):
        self.mutate("crates/jojo-helper/src/main.rs", '\nuse std::{fs};\nfn forbidden() { fs::read("/not-a-real-credential-fixture"); }\n')

    def test_build_hook_is_rejected(self):
        (self.root / "crates/jojo-helper/build.rs").write_text("fn main() {}\n")
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_test_support_cannot_be_linked_into_helper(self):
        p = self.root / "crates/jojo-helper/Cargo.toml"
        p.write_text(p.read_text().replace('[dependencies]', '[dependencies]\njojo-test-support = { path = "../jojo-test-support" }'))
        self.assertTrue(CHECK.boundary_errors(self.root))

    # P1-02: the capabilities opened for storage stay confined to their allowlisted places.
    def test_unsafe_outside_the_sqlite_binding_is_rejected(self):
        self.mutate("crates/jojo-storage/src/tx.rs", '\nfn forbidden() { unsafe { std::hint::unreachable_unchecked() } }\n')

    def test_ffi_in_the_helper_is_rejected(self):
        self.mutate("crates/jojo-helper/src/lib.rs", '\nunsafe extern "C" { fn getpid() -> i32; }\n')

    def test_child_process_in_storage_production_code_is_rejected(self):
        self.mutate("crates/jojo-storage/src/engine.rs", '\nfn forbidden() { std::process::Command::new("codex"); }\n')

    def test_tests_cannot_launch_arbitrary_programs(self):
        self.mutate("crates/jojo-storage/tests/t02_crash.rs", '\nfn forbidden() { Command::new("claude"); }\n')
        self.mutate("crates/jojo-helper/tests/t02_helper_ownership.rs", '\nfn forbidden() { Command::new("/usr/bin/git"); }\n')

    def test_ambient_time_in_storage_is_rejected(self):
        self.mutate("crates/jojo-storage/src/writer.rs", '\nfn forbidden() { std::time::SystemTime::now(); }\n')

    def test_ambient_environment_in_storage_is_rejected(self):
        self.mutate("crates/jojo-storage/src/ownership.rs", '\nfn forbidden() { std::env::var_os("HOME"); }\n')

    def test_network_in_storage_is_rejected(self):
        self.mutate("crates/jojo-storage/src/engine.rs", '\nuse std::os::unix::net::UnixListener;\n')

    def test_external_storage_dependency_is_rejected(self):
        p = self.root / "crates/jojo-storage/Cargo.toml"
        p.write_text(p.read_text().replace('[dependencies]', '[dependencies]\nunapproved = "1"'))
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_weakened_storage_lints_are_rejected(self):
        p = self.root / "crates/jojo-storage/Cargo.toml"
        p.write_text(p.read_text().replace('[lints]\nworkspace = true', '[lints.rust]\nunsafe_code = "allow"'))
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_filesystem_access_in_test_support_is_rejected(self):
        self.mutate("crates/jojo-test-support/src/lib.rs", '\nfn forbidden() { let _ = std::fs::read("/x"); }\n')

    def test_storage_build_hook_is_rejected(self):
        (self.root / "crates/jojo-storage/build.rs").write_text("fn main() {}\n")
        self.assertTrue(CHECK.boundary_errors(self.root))

    def test_swift_remote_package_is_rejected(self):
        self.mutate("macos/JojoApp/Package.swift", '\n// temporary fixture\nlet forbidden = .package(url: "https://example.invalid/repo", from: "1.0.0")\n')

    def test_swift_gui_database_access_is_rejected(self):
        self.mutate("macos/JojoApp/Sources/JojoCore/QuitCoordinator.swift", "\nimport SQLite3\nlet forbidden = sqlite3_open\n")

    def test_swift_gui_process_signal_is_rejected(self):
        self.mutate("macos/JojoApp/Sources/JojoCore/HelperProcess.swift", "\nfunc forbidden(_ p: Process) { p.terminate() }\n")

    def test_swift_gui_kill_by_pid_is_rejected(self):
        self.mutate("macos/JojoApp/Sources/JojoApp/AppDelegate.swift", "\nfunc forbidden(_ pid: Int32) { _ = kill(pid, SIGKILL) }\n")

    def test_swift_process_launch_outside_helper_transport_is_rejected(self):
        self.mutate("macos/JojoApp/Sources/JojoCore/QuitCoordinator.swift", "\nlet forbidden = Process()\n")

    def test_swift_test_target_may_not_signal(self):
        self.mutate("macos/JojoApp/Tests/JojoCoreTests/FakeHelper.swift", "\nfunc forbidden(_ pid: Int32) { _ = kill(pid, 9) }\n")

    def test_local_git_surface_rejects_provider_executable(self):
        self.mutate("crates/jojo-storage/src/worktrees.rs", '\nfn forbidden() { Command::new("codex"); }\n')

    def test_local_git_surface_rejects_shell(self):
        self.mutate("crates/jojo-storage/src/worktrees.rs", '\nfn forbidden() { Command::new("/bin/sh"); }\n')

    def test_local_git_surface_does_not_extend_other_storage_modules(self):
        self.mutate("crates/jojo-storage/src/tx.rs", '\nfn forbidden() { Command::new("/usr/bin/git"); }\n')

    def test_local_git_fixture_rejects_provider_executable(self):
        self.mutate("crates/jojo-storage/tests/t07_worktrees.rs", '\nfn forbidden() { Command::new("claude"); }\n')

    def test_process_surface_rejects_named_provider_executable(self):
        self.mutate("crates/jojo-storage/src/process.rs", '\nfn forbidden() { Command::new("claude"); }\n')

    def test_process_surface_rejects_unlisted_system_tool(self):
        self.mutate("crates/jojo-storage/src/process.rs", '\nfn forbidden() { let _ = system("/usr/bin/curl", &[]); }\n')

    def test_process_surface_rejects_pre_exec_and_inherited_environment(self):
        p = self.root / "crates/jojo-storage/src/process.rs"
        p.write_text(p.read_text().replace(".env_clear()", "", 1))
        self.assertTrue(CHECK.boundary_errors(self.root))
        self.mutate("crates/jojo-storage/src/process.rs", '\nfn forbidden(c: &mut Command) { c.uid(0); }\n')

    def test_process_surface_guard_launch_must_be_the_guard_mode_with_a_cleared_environment(self):
        p = self.root / "crates/jojo-storage/src/process.rs"
        source = p.read_text()
        self.assertEqual(len(CHECK.re.findall(r"Command::new\(guard\)\s*\.arg\(GUARD_ARGUMENT\)", source)), 1)
        # The guard executable started without its fixed first argument is any program.
        p.write_text(CHECK.re.sub(r"(Command::new\(guard\))\s*\.arg\(GUARD_ARGUMENT\)", r"\1", source))
        self.assertTrue(CHECK.boundary_errors(self.root))
        p.write_text(source)
        self.assertEqual(CHECK.boundary_errors(self.root), [])
        # A second guard-shaped launch site is not covered by the one that was reviewed.
        self.mutate("crates/jojo-storage/src/process.rs",
                    '\nfn forbidden(guard: &Path) { Command::new(guard).arg(GUARD_ARGUMENT).env_clear(); }\n')

    def test_helper_guard_mode_may_not_build_a_launch_or_read_the_environment(self):
        self.mutate("crates/jojo-helper/src/main.rs", '\nfn forbidden() { std::env::var("PATH").ok(); }\n')

    def test_process_surface_does_not_extend_to_launch_journal(self):
        self.mutate("crates/jojo-storage/src/launch.rs", '\nfn forbidden() { std::process::Command::new("/bin/ps"); }\n')

    def test_fixture_may_not_launch_other_programs_or_read_environment(self):
        self.mutate("crates/jojo-storage/src/bin/jojo-process-fixture.rs", '\nfn forbidden() { Command::new("/usr/bin/codex"); }\n')

    def test_fixture_may_not_read_environment(self):
        self.mutate("crates/jojo-storage/src/bin/jojo-process-fixture.rs", '\nfn forbidden() { std::env::var("HOME").ok(); }\n')

    def test_production_code_may_not_name_fixture(self):
        self.mutate("crates/jojo-storage/src/launch.rs", '\nconst FORBIDDEN: &str = "jojo-process-fixture";\n')

    def test_t09_tests_may_launch_only_git(self):
        self.mutate("crates/jojo-storage/tests/t09_process.rs", '\nfn forbidden() { Command::new("/bin/sh"); }\n')

    def test_t10_tests_may_launch_only_git(self):
        self.mutate("crates/jojo-storage/tests/t10_results.rs", '\nfn forbidden() { Command::new("/usr/bin/codex"); }\n')

    def test_settlement_module_launches_nothing(self):
        self.mutate("crates/jojo-storage/src/outcomes.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/git"); }\n')

    def test_validation_module_launches_nothing(self):
        self.mutate("crates/jojo-storage/src/validation.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/git"); }\n')

    def test_evidence_store_launches_nothing(self):
        self.mutate("crates/jojo-storage/src/evidence.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/git"); }\n')

    def test_t11_tests_may_launch_only_git(self):
        self.mutate("crates/jojo-storage/tests/t11_recovery.rs", '\nfn forbidden() { Command::new("/bin/kill"); }\n')

    def test_recovery_module_launches_nothing(self):
        self.mutate("crates/jojo-storage/src/recovery.rs", '\nfn forbidden() { std::process::Command::new("/bin/ps"); }\n')

    def test_shutdown_module_launches_nothing(self):
        self.mutate("crates/jojo-storage/src/shutdown.rs", '\nfn forbidden() { std::process::Command::new("/bin/kill"); }\n')

    def test_shared_attempt_fixture_may_launch_only_git(self):
        self.mutate("crates/jojo-storage/tests/common/attempts.rs", '\nfn forbidden() { Command::new("/bin/kill"); }\n')

    def test_t12_tests_launch_nothing_themselves(self):
        self.mutate("crates/jojo-storage/tests/t12_shutdown.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/git"); }\n')

    def test_ipc_service_cannot_signal_or_launch_directly(self):
        self.mutate("crates/jojo-helper/src/service.rs", '\nfn forbidden() { std::process::Command::new("/bin/kill"); }\n')

    def test_ipc_queries_cannot_launch_processes(self):
        self.mutate("crates/jojo-storage/src/ipc_queries.rs", '\nfn forbidden() { std::process::Command::new("/bin/ps"); }\n')

    def test_t13_runtime_tests_cannot_launch_directly(self):
        self.mutate("crates/jojo-storage/tests/t13_runtime.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/codex"); }\n')

    def test_codex_fixture_may_not_launch_programs(self):
        self.mutate("crates/jojo-storage/src/bin/jojo-codex-fixture.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/codex"); }\n')

    def test_codex_fixture_may_not_read_ambient_variables(self):
        self.mutate("crates/jojo-storage/src/bin/jojo-codex-fixture.rs", '\nfn forbidden() { std::env::var("HOME").ok(); }\n')

    def test_codex_fixture_may_not_touch_files(self):
        self.mutate("crates/jojo-storage/src/bin/jojo-codex-fixture.rs", '\nfn forbidden() { std::fs::read("auth").ok(); }\n')

    def test_production_code_may_not_name_codex_fixture(self):
        self.mutate("crates/jojo-storage/src/codex.rs", '\nconst FORBIDDEN: &str = "jojo-codex-fixture";\n')

    def test_codex_adapter_launches_nothing(self):
        self.mutate("crates/jojo-storage/src/codex.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/codex"); }\n')

    def test_codex_adapter_may_not_read_ambient_environment(self):
        self.mutate("crates/jojo-storage/src/codex.rs", '\nfn forbidden() { std::env::var_os("CODEX_HOME"); }\n')

    def test_codex_adapter_may_not_name_credential_stores_or_install_locations(self):
        for forbidden in ['"auth.json"', '"/opt/homebrew/bin/codex"', '".codex"', '"Keychain"']:
            self.setUp()
            self.mutate("crates/jojo-storage/src/codex.rs", f'\nconst FORBIDDEN: &str = {forbidden};\n')

    def test_production_code_may_not_name_provider_credential_variables(self):
        for forbidden in ['"OPENAI_API_KEY"', '"CODEX_API_KEY"', '"ANTHROPIC_API_KEY"']:
            self.setUp()
            self.mutate("crates/jojo-domain/src/codex.rs", f'\nconst FORBIDDEN: &str = {forbidden};\n')

    def test_codex_contract_stays_pure(self):
        self.mutate("crates/jojo-domain/src/codex.rs", '\nfn forbidden() { std::fs::read("x").ok(); }\n')

    def test_claude_contract_and_adapter_stay_offline(self):
        self.mutate("crates/jojo-domain/src/claude.rs", '\nfn forbidden() { std::fs::read("x").ok(); }\n')
        for forbidden in ['Command::new("claude");', 'std::env::var("HOME").ok();',
                          'let _ = "ANTHROPIC_API_KEY";', 'let _ = "/opt/homebrew/bin/claude";']:
            self.setUp()
            self.mutate("crates/jojo-storage/src/claude.rs", f'\nfn forbidden() {{ {forbidden} }}\n')

    def test_codex_envelope_cannot_be_widened_in_the_process_surface(self):
        self.mutate("crates/jojo-storage/src/process.rs", '\nfn forbidden() { Command::new("codex"); }\n')

    def test_helper_cannot_build_os_requests_outside_the_adapter(self):
        for forbidden in ['ProcessLaunch::from_spec(&spec, vec![], vec![]).ok();',
                          'jojo_domain::codex::exec_argv(std::path::Path::new("/x")).ok();',
                          'jojo_domain::claude::print_argv();']:
            self.setUp()
            self.mutate("crates/jojo-helper/src/service.rs", f'\nfn forbidden(spec: LaunchSpec) {{ {forbidden} }}\n')

    def test_helper_slice_commands_cannot_touch_files(self):
        self.mutate("crates/jojo-helper/src/service.rs", '\nfn forbidden() { std::fs::read("/etc/passwd").ok(); }\n')

    def test_helper_cannot_read_provider_variables(self):
        self.mutate("crates/jojo-helper/src/service.rs", '\nfn forbidden() { std::env::var("CODEX_HOME").ok(); }\n')

    def test_slice_orchestration_launches_nothing(self):
        self.mutate("crates/jojo-storage/src/slice.rs", '\nfn forbidden() { std::process::Command::new("/usr/bin/git"); }\n')

    def test_slice_orchestration_reads_no_ambient_environment(self):
        self.mutate("crates/jojo-storage/src/slice.rs", '\nfn forbidden() { std::env::var_os("HOME"); }\n')

    def test_real_helper_test_can_launch_only_the_workspace_helper(self):
        relative = "crates/jojo-storage/tests/p2_03_real_helper.rs"
        self.mutate(relative, '\nfn other() { std::process::Command::new("/bin/sh"); }\n')

    def test_real_helper_test_cannot_redirect_the_helper_path(self):
        relative = "crates/jojo-storage/tests/p2_03_real_helper.rs"
        self.mutate(relative, '\nfn other() { let helper = "/bin/sh"; std::process::Command::new(&helper); }\n')

    def test_validation_runner_has_no_ambient_clock_or_launch_api(self):
        relative = "crates/jojo-storage/src/validator.rs"
        self.mutate(relative, "\nfn late() -> std::time::Instant { std::time::Instant::now() }\n")
        shutil.copy2(ROOT / relative, self.root / relative)
        self.mutate(relative, '\nfn run() { std::process::Command::new("/bin/sh"); }\n')

    def test_helper_validation_commands_cannot_touch_files(self):
        self.mutate("crates/jojo-helper/src/service.rs", '\nfn report() { let _ = std::fs::read("/tmp/report"); }\n')

    def test_gui_start_controls_cannot_launch_or_signal(self):
        self.mutate("macos/JojoApp/Sources/JojoCore/Workspace.swift", '\nfunc forbidden() { let p = Process(); _ = p }\n')
        self.setUp()
        self.mutate("macos/JojoApp/Sources/JojoApp/Views.swift", '\nfunc forbidden() { kill(1, SIGKILL) }\n')


if __name__ == "__main__":
    unittest.main()
