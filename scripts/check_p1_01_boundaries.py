#!/usr/bin/env python3
"""Fail-closed architecture guard for the deliberately small Jojo source surface.

Introduced in P1-01 and amended in P1-02 (the file name is kept for continuity). P1-02
adds exactly one crate, `jojo-storage`, and scopes three capabilities that P1-01 banned
everywhere:

* filesystem access: only `jojo-storage` and the helper's integration tests;
* `unsafe`/FFI: forbidden in workspace source; SQLite uses rusqlite;
* child processes: only integration tests of `jojo-storage`/`jojo-helper`, and only to
  launch the test binary itself or the workspace's own `jojo-helper` executable.

P1-03.5 approves pinned rusqlite, sha2, serde and serde_json dependencies.
P1-07 permits only /usr/bin/git in storage worktrees.rs and its T07 fixture.
P1-08 adds pure provider DTOs and test-only synthetic adapters.
P1-09 permits exactly one supervised launch surface (storage process.rs: the LaunchSpec
executable plus /bin/ps, /bin/kill and /usr/sbin/sysctl, all with a cleared environment
and only `process_group` from CommandExt) and one test-only fake executable
(storage src/bin/jojo-process-fixture.rs) that may relaunch itself or /bin/sh. No
production source may name that fixture; T09 tests may launch /usr/bin/git only.
P1-10 adds no launch surface: changed-path evidence reuses the storage worktrees.rs Git
boundary, and T10 tests (like T09) may launch /usr/bin/git only.
P1-11 adds no launch surface: reopen inspection reuses process.rs (/bin/ps, sysctl) and
the worktrees.rs Git boundary, and T11 tests (like T09/T10) may launch /usr/bin/git only.
P1-12 adds no launch or signal surface: the shutdown coordinator only commits stop intents
that P1-09 runners act on. The T11 attempt fixture moved to tests/common/attempts.rs (shared
with T12) and keeps the /usr/bin/git-only rule; T12 tests launch nothing themselves.
P2-01 adds the offline Codex adapter (domain codex.rs: pure contract; storage codex.rs:
executable/home validation and translation) and no launch surface: the adapter may not
start a process, read the ambient environment, or name a credential store, an install
location or any provider variable other than CODEX_HOME. Its test-only stand-in
(storage src/bin/jojo-codex-fixture.rs) launches nothing and may read only its arguments
and the environment it was given. No production source may name it.
P2-02 adds no launch surface either: Start reaches the unchanged P1-09 launch only through
the Codex adapter. The helper gains no filesystem or process API (repository and worktree
root inspection live in storage slice.rs), and the fixture may additionally write the one
developed file in its working directory (`std::fs::write` only).
P2-03 adds the host validation runner (storage validator.rs). It has no launch API of its
own: it builds a request for the unchanged P1-09 supervised runner, its only executable is
the OS sandbox wrapper, and it measures budgets with the injected clock. The helper still
has no filesystem or process API. One storage test (p2_03_real_helper.rs) may launch the
workspace's own jojo-helper.
P2-04 adds no production source. The Codex fixture gains the rehearsal scenarios for the
generated sample repository: still `std::fs::write` in its working directory only, and it
still launches nothing.
P2-04.1 adds the agent guard to the P1-09 process surface (storage process.rs): a third
launch site that starts only the configured guard executable (the helper itself, argument
`--agent-guard`) inside the owned process group, with a cleared environment. The guard
signals only the group it has proven it is a member of. The helper executable gains the
`--agent-guard` mode and reads its own path; it still builds no OS request.
M1 adds the Claude Code adapter beside the Codex one, under the same rules (domain
claude.rs: pure contract; storage claude.rs: executable/home validation and translation)
and no launch surface. Its provider variables are HOME and USER, passed as values the
helper's launcher configured; it names no credential store, install location or other
provider variable. The Codex fixture also speaks the Claude Code stream dialect; it gains no
file or process operation. The helper still builds no OS request and no argument vector.
M5 adds one launch surface, storage remote.rs: `/usr/bin/git` (push of one commit to one
branch, never forced; ls-remote; config --get) and the `gh` discovery found (pr create
--draft, pr list, auth status), each launched once, with a cleared environment, no
CommandExt and no force, mirror, delete or tag argument. It reads no token: git and gh use
their own sign-in in the HOME the helper was given. The Codex fixture's `gh…` copy may also
create, read and append files under that HOME (`$HOME/fake-gh`), and finds HOME through
`std::env::vars()`.
Effect boundaries remain unchanged: no network/live-provider/auth integration,
no build scripts, no ambient clock in the domain (and now none in storage either).

This is an allowlist/regression check, not an adversarial Rust/Swift security verifier.
It scans Jojo code only, never the audited upstream source or provider home directories.
"""
from pathlib import Path
import re
import sys
import tomllib

DOMAIN = {"jojo-domain": {"path": "../jojo-domain"}}
STORAGE = {"jojo-storage": {"path": "../jojo-storage"}}
TEST_SUPPORT = {"jojo-test-support": {"path": "../jojo-test-support"}}
# crate -> (production dependencies, dev-dependencies)
CRATES = {
    "jojo-domain": ({"serde": "=1.0.228", "serde_json": "=1.0.145"}, TEST_SUPPORT),
    "jojo-helper": (DOMAIN | STORAGE, {}),
    "jojo-storage": (DOMAIN | {"rusqlite": {"version": "=0.37.0", "features": ["extra_check"]}, "sha2": "=0.10.9"}, TEST_SUPPORT | {"jojo-helper": {"path": "../jojo-helper"}}),
    "jojo-test-support": (DOMAIN, {}),
}
WORKSPACE_LINTS = {"workspace": True}
FILESYSTEM_CRATES = {"jojo-storage"}
PROCESS_TEST_CRATES = {"jojo-storage", "jojo-helper"}

ALWAYS_FORBIDDEN = [
    r"\b(?:TcpStream|TcpListener|UdpSocket|UnixStream|UnixListener|SocketAddr|URLSession)\b",
    r"\bstd\s*::\s*net\b",
    r"\b(?:include|include_str|include_bytes|asm|global_asm)\s*!",
    r"#\s*\[\s*(?:path|no_mangle)\b",
    r"\b(?:reqwest|tokio|sqlx|ureq|libc)\b",
]
FILESYSTEM = r"\bstd\s*::\s*fs\b|\bfs\s*::|\bFile\b|\bOpenOptions\b|\bDirBuilder\b"
FFI = r"\b(?:unsafe|extern)\b|#\s*\[\s*link\b"
PROCESS = r"\b(?:Command|CommandExt)\b"
# The only programs a test may launch: itself, or the helper built from this workspace.
ALLOWED_PROGRAM = r'(?:std::env::current_exe\(\)\.unwrap\(\)|env!\("CARGO_BIN_EXE_jojo-helper"\))'
# P2-03: the one storage test that drives the real helper executable. It may launch only the
# workspace's own jojo-helper, found next to the fixture binary Cargo built for this test.
REAL_HELPER_TEST = "crates/jojo-storage/tests/p2_03_real_helper.rs"
REAL_HELPER = r'let helper = Path::new\(CODEX_FIXTURE\)\.with_file_name\("jojo-helper"\);'
PROCESS_SURFACE = "crates/jojo-storage/src/process.rs"
# D3: executable discovery starts two programs, each in one reviewed shape: the validated
# login shell with the fixed PATH script, and a found CLI with `--version`.
DISCOVERY_SURFACE = "crates/jojo-storage/src/discovery.rs"
DISCOVERY_SCRIPT = 'const SCRIPT: &str = "printf \'\\\\n__JOJO_PATH__%s__JOJO_PATH__\\\\n\' \\"$PATH\\"";'
DISCOVERY_LAUNCHES = [
    r'Command::new\(shell\);\s*launch\s*\.args\(\["-ilc", SCRIPT\]\)\s*\.env_clear\(\)\s*\.envs\(\[\("HOME", home\)\]\)',
    r'Command::new\(executable\);\s*launch\s*\.arg\("--version"\)\s*\.env_clear\(\)',
]
# The one install layout discovery may name: Codex's npm launcher and its platform binary.
DISCOVERY_LAYOUT = '"node_modules/@openai"'
# M5: the one surface that reaches a remote, in two reviewed launch shapes.
REMOTE_SURFACE = "crates/jojo-storage/src/remote.rs"
REMOTE_LAUNCHES = [r'Command::new\("/usr/bin/git"\);', r'Command::new\(gh\);']
REMOTE_FORBIDDEN = r'"(?:--force[^"]*|-f|--mirror|--delete|-d|--tags|--all|--prune)"|"\+'
PROCESS_FIXTURE = "crates/jojo-storage/src/bin/jojo-process-fixture.rs"
CODEX_FIXTURE = "crates/jojo-storage/src/bin/jojo-codex-fixture.rs"
FIXTURES = {PROCESS_FIXTURE, CODEX_FIXTURE}
# Credential stores, real install locations and provider variables other than CODEX_HOME.
PROVIDER_SECRETS = (r'auth\.json|\bKeychain\b|\bSecItem\w*|/opt/homebrew|node_modules|[\x22\x27/~]\.codex\b'
                    r'|"(?:OPENAI_\w+|ANTHROPIC_\w+|CODEX_(?!HOME")\w+)"')
SYSTEM_TOOLS = {"/bin/ps", "/bin/kill", "/usr/sbin/sysctl"}
GIT_SURFACES = {"crates/jojo-storage/src/worktrees.rs", "crates/jojo-storage/tests/t07_worktrees.rs",
                "crates/jojo-storage/tests/t09_process.rs",
                "crates/jojo-storage/tests/t10_results.rs",
                "crates/jojo-storage/tests/t11_recovery.rs",
                "crates/jojo-storage/tests/common/attempts.rs"}


def process_surface_errors(relative: str, code: str) -> list[str]:
    """The supervisor launches only the spec executable, three fixed system tools and its guard."""
    errors = []
    remainder = re.sub(r"Command::new\(\s*(?:program|&launch\.executable)\s*\)", "", code)
    # P2-04.1: one more launch site, the guard (the helper's own executable) placed inside
    # the owned process group with a fixed argument vector and nothing inherited.
    remainder = re.sub(r"Command::new\(guard\)\s*\.arg\(GUARD_ARGUMENT\)", "", remainder, count=1)
    tools = set(re.findall(r'\bsystem\(\s*"([^"]*)"', code))
    if "Command::new" in remainder or tools - SYSTEM_TOOLS or re.search(r"\bpub\s+fn\s+system\b", code):
        errors.append(f"{relative}: process surface may launch only the spec executable and fixed system tools")
    if code.count(".env_clear()") != 3 or re.search(r"\.(?:exec|pre_exec|uid|gid|arg0)\s*\(", code):
        errors.append(f"{relative}: launches must clear the environment and use only process_group")
    return errors


def discovery_surface_errors(relative: str, code: str) -> list[str]:
    """Discovery launches only the login shell with the PATH script and a CLI with --version."""
    remainder = code
    for shape in DISCOVERY_LAUNCHES:
        if len(re.findall(shape, remainder)) != 1:
            return [f"{relative}: discovery may launch only the login shell script and --version"]
        remainder = re.sub(shape, "", remainder)
    if ("Command::new" in remainder or "CommandExt" in code or DISCOVERY_SCRIPT not in code
            or re.search(r"\.(?:args?|env|exec|pre_exec|uid|gid|arg0|process_group|current_dir)\s*\(", remainder)):
        return [f"{relative}: discovery may launch only the login shell script and --version"]
    return []


def remote_surface_errors(relative: str, code: str) -> list[str]:
    """The remote surface launches git and gh once each, cleared, never forced."""
    remainder = code
    for shape in REMOTE_LAUNCHES:
        if len(re.findall(shape, remainder)) != 1:
            return [f"{relative}: remote surface may launch only /usr/bin/git and the found gh"]
        remainder = re.sub(shape, "", remainder)
    if ("Command::new" in remainder or "CommandExt" in code or code.count(".env_clear()") != 2
            or re.search(REMOTE_FORBIDDEN, code)
            or re.search(r"\.(?:exec|pre_exec|uid|gid|arg0|process_group)\s*\(", code)):
        return [f"{relative}: remote surface may launch only /usr/bin/git and the found gh"]
    return []


def fixture_errors(relative: str, code: str) -> list[str]:
    errors = []
    remainder = re.sub(r'Command::new\(\s*(?:me\(\)|"/bin/sh")\s*\)', "", code)
    if "Command::new" in remainder:
        errors.append(f"{relative}: fixture may relaunch only itself or /bin/sh")
    if re.sub(r"std::env::(?:args|current_exe)\(\)", "", code).count("env"):
        errors.append(f"{relative}: fixture may read only its arguments and path")
    return errors


def codex_fixture_errors(relative: str, code: str) -> list[str]:
    errors = []
    # P2-02: the one permitted file operation is writing the developed file in its cwd.
    # M5: the fake gh keeps its pull requests under its HOME.
    files = re.sub(r"std::fs::(?:write|read_to_string|create_dir_all)\(|std::fs::OpenOptions::new\(\)", "", code)
    if re.search(PROCESS, code) or re.search(FILESYSTEM, files):
        errors.append(f"{relative}: Codex fixture may not launch a program or touch a file")
    if re.sub(r"std::env::(?:args|vars)\(\)", "", code).count("env"):
        errors.append(f"{relative}: Codex fixture may read only its arguments and given environment")
    return errors


def source_errors(root: Path, name: str, path: Path) -> list[str]:
    relative = path.relative_to(root).as_posix()
    # Comments are ignored; API names in executable source remain checked.
    code = re.sub(r"/\*.*?\*/|//[^\n]*", "", path.read_text(), flags=re.S)
    is_test = relative.startswith(f"crates/{name}/tests/")
    errors = []
    if any(re.search(pattern, code) for pattern in ALWAYS_FORBIDDEN):
        errors.append(f"{relative}: forbidden network/integration/build surface")
    if re.search(FFI, code):
        errors.append(f"{relative}: unsafe/FFI is forbidden in workspace source")
    if re.search(FILESYSTEM, code) and not (name in FILESYSTEM_CRATES or (is_test and name in PROCESS_TEST_CRATES)):
        errors.append(f"{relative}: forbidden filesystem access")
    if relative == PROCESS_FIXTURE:
        errors.extend(fixture_errors(relative, code))
    elif relative == CODEX_FIXTURE:
        errors.extend(codex_fixture_errors(relative, code))
    elif re.search(r"jojo-(?:process|codex)-fixture", code) and not is_test:
        errors.append(f"{relative}: production code must not name the test fixture executable")
    named = code.replace(DISCOVERY_LAYOUT, "", 1) if relative == DISCOVERY_SURFACE else code
    if not is_test and relative not in FIXTURES and re.search(PROVIDER_SECRETS, named):
        errors.append(f"{relative}: provider credential store, install location or variable named in production")
    if re.search(PROCESS, code) and relative == PROCESS_SURFACE:
        errors.extend(process_surface_errors(relative, code))
    elif re.search(PROCESS, code) and relative == PROCESS_FIXTURE:
        pass
    elif relative == DISCOVERY_SURFACE:
        errors.extend(discovery_surface_errors(relative, code))
    elif relative == REMOTE_SURFACE:
        errors.extend(remote_surface_errors(relative, code))
    elif re.search(PROCESS, code) and relative == REAL_HELPER_TEST:
        remainder = re.sub(r"Command::new\(&helper\)", "", code)
        if ("Command::new" in remainder or "CommandExt" in code or len(re.findall(REAL_HELPER, code)) != 1
                or len(re.findall(r"\bhelper\s*=[^=]", code)) != 1):
            errors.append(f"{relative}: may launch only the workspace jojo-helper built next to the fixture")
    elif re.search(PROCESS, code):
        git_surface = relative in GIT_SURFACES
        if git_surface:
            remainder = re.sub(r'Command::new\(\s*"/usr/bin/git"\s*\)', "", code)
            if "Command::new" in remainder or "CommandExt" in code:
                errors.append(f"{relative}: local Git boundary permits only /usr/bin/git")
        elif not (is_test and name in PROCESS_TEST_CRATES):
            errors.append(f"{relative}: forbidden child-process launch")
        elif re.sub(rf"Command::new\(\s*{ALLOWED_PROGRAM}\s*\)", "", code).count("Command::new"):
            errors.append(f"{relative}: tests may launch only the test binary or jojo-helper")
    if name in {"jojo-domain", "jojo-storage"} and not is_test and relative not in FIXTURES:
        # `#[cfg(test)] mod tests { … }` blocks may use a temporary directory; every other
        # rule above still applies to them in full, and code outside them is production.
        production = re.sub(r"#\[cfg\(test\)\]\nmod tests \{.*?\n\}\n", "", code, flags=re.S)
        if re.search(r"\b(?:SystemTime|Instant)\b", production):
            errors.append(f"{relative}: ambient clock access (time must be injected)")
        if re.search(r"\benv\b", production):
            errors.append(f"{relative}: ambient environment access")
    if name == "jojo-helper" and not is_test and re.search(r"\b(?:from_spec|Supervised|exec_argv|print_argv)\b", code):
        # P2-02, M1: Start may reach a process only through an adapter's prepare_launch and
        # the existing worker registry; the helper never builds an OS request itself.
        errors.append(f"{relative}: helper must launch only through the provider adapter")
    if name == "jojo-helper":
        reads = re.findall(r'\bvar(?:_os)?\s*\(\s*([^)]*)\)', code)
        if any(value.strip() != '"HOME"' for value in reads) or re.search(r"\b(?:vars|vars_os|set_var|remove_var)\s*\(", code):
            errors.append(f"{relative}: unapproved environment access")
        allowed_compile = {'"CARGO_PKG_VERSION"'} | ({'"CARGO_BIN_EXE_jojo-helper"'} if is_test else set())
        compile_reads = re.findall(r'\b(?:env|option_env)\s*!\s*\(\s*([^)]*)\)', code)
        if any(value.strip() not in allowed_compile for value in compile_reads):
            errors.append(f"{relative}: unapproved compile-time environment access")
    return errors


def boundary_errors(root: Path) -> list[str]:
    errors = []
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    expected_members = {f"crates/{name}" for name in CRATES}
    if set(workspace.get("workspace", {}).get("members", [])) != expected_members:
        errors.append("workspace members differ from the allowlist")
    if set(workspace) != {"workspace"} or set(workspace["workspace"]) != {"members", "resolver", "package", "lints"}:
        errors.append("unexpected workspace build/dependency configuration")
    if workspace["workspace"].get("package") != {"version": "0.1.0", "edition": "2024", "rust-version": "1.95", "publish": False}:
        errors.append("workspace publication/toolchain baseline changed")
    if workspace["workspace"].get("lints") != {"rust": {"unsafe_code": "forbid"}, "clippy": {"all": "warn"}}:
        errors.append("workspace lints changed (unsafe code must stay forbidden by default)")
    if tomllib.loads((root / ".cargo/config.toml").read_text()) != {"net": {"offline": True}}:
        errors.append("Cargo must remain offline with no wrapper/alias overrides")
    actual_manifests = {p.parent.name for p in (root / "crates").rglob("Cargo.toml")}
    if actual_manifests != set(CRATES):
        errors.append("unexpected crate manifest")
    for name, (dependencies, dev_dependencies) in sorted(CRATES.items()):
        directory = root / "crates" / name
        if not (directory / "Cargo.toml").exists():
            continue
        manifest = tomllib.loads((directory / "Cargo.toml").read_text())
        if set(manifest) - {"package", "dependencies", "dev-dependencies", "lints"}:
            errors.append(f"{name}: unexpected target/features/build configuration")
        if manifest.get("dependencies", {}) != dependencies:
            errors.append(f"{name}: forbidden production dependencies")
        if manifest.get("dev-dependencies", {}) != dev_dependencies:
            errors.append(f"{name}: test-support boundary changed")
        if manifest.get("lints") != WORKSPACE_LINTS:
            errors.append(f"{name}: lint configuration weakened")
        package = manifest["package"]
        if set(package) != {"name", "version", "edition", "rust-version", "publish"}:
            errors.append(f"{name}: unexpected package/build hook configuration")
        if package.get("name") != name or package.get("publish") != {"workspace": True}:
            errors.append(f"{name}: package identity/publication boundary changed")
        for p in directory.rglob("*"):
            if not p.is_file():
                continue
            if p.name == "build.rs":
                errors.append(f"{name}: build scripts are not allowed")
            if p.suffix == ".rs":
                errors.extend(source_errors(root, name, p))
    swift_root = root / "macos/JojoApp"
    manifest = (swift_root / "Package.swift").read_text()
    if re.search(r"\.package\s*\(|https?://", manifest):
        errors.append("Swift package cannot add remote dependencies")
    # P1-14: the GUI never owns SQLite, networking or file administration, never signals a
    # process and launches exactly one program (the helper) from one reviewed transport file.
    for p in [*(swift_root / "Sources").rglob("*.swift"), *(swift_root / "Tests").rglob("*.swift")]:
        code = re.sub(r"//[^\n]*", "", p.read_text())
        relative = p.relative_to(swift_root).as_posix()
        if re.search(r"\b(?:URLSession|FileManager|Network|SQLite3|sqlite3_\w+|NSTask)\b", code):
            errors.append(f"{p.relative_to(root)}: GUI database/network/file administration")
        if re.search(r"(?<![.\w])(?:kill|killpg|raise|posix_spawn\w*|system|execv\w*)\s*\(|\bSIG(?:KILL|TERM|INT|HUP|QUIT|STOP)\b|\.(?:terminate|interrupt|suspend)\(\s*\)", code):
            errors.append(f"{p.relative_to(root)}: GUI may not signal processes; the Rust runner owns stops")
        if re.search(r"\bProcess\b", code) and relative != "Sources/JojoCore/HelperProcess.swift":
            errors.append(f"{p.relative_to(root)}: only the helper transport may launch a process")
        if re.search(r"@main|WindowGroup|\bWindow\s*\(", code) and not relative.startswith("Sources/JojoApp/"):
            errors.append(f"{p.relative_to(root)}: scenes belong only to the application target")
    lock = root / "Cargo.lock"
    if lock.exists():
        packages = tomllib.loads(lock.read_text()).get("package", [])
        if sorted(packages, key=lambda p: p["name"]) != APPROVED_PACKAGES:
            errors.append("lockfile differs from approved pinned dependency graph")
    return errors


# Exact reviewed graph: versions, sources, checksums and dependency edges.
APPROVED_PACKAGES = [{'checksum': '3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06',
  'name': 'bitflags',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '2.13.2'},
 {'checksum': '3078c7629b62d3f0439517fa394996acacc5cbc91c5a20d8c658e77abd503a71',
  'dependencies': ['generic-array'],
  'name': 'block-buffer',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.10.4'},
 {'checksum': '4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600',
  'name': 'cfg-if',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.5'},
 {'checksum': '59ed5838eebb26a2bb2e58f6d5b5316989ae9d08bab10e0e6d103e656d1b0280',
  'dependencies': ['libc'],
  'name': 'cpufeatures',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.2.17'},
 {'checksum': '78c8292055d1c1df0cce5d180393dc8cce0abec0a7102adb6c7b1eef6016d60a',
  'dependencies': ['generic-array', 'typenum'],
  'name': 'crypto-common',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.1.7'},
 {'checksum': '9ed9a281f7bc9b7576e61468ba615a66a5c8cfdff42420a70aa82701a3b1e292',
  'dependencies': ['block-buffer', 'crypto-common'],
  'name': 'digest',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.10.7'},
 {'checksum': '2acce4a10f12dc2fb14a218589d4f1f62ef011b2d0cc4b3cb1bba8e94da14649',
  'name': 'fallible-iterator',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.3.0'},
 {'checksum': '7360491ce676a36bf9bb3c56c1aa791658183a54d2744120f27285738d90465a',
  'name': 'fallible-streaming-iterator',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.1.9'},
 {'checksum': 'd9c4f5dac5e15c24eb999c26181a6ca40b39fe946cbe4c263c7209467bc83af2',
  'name': 'foldhash',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.1.5'},
 {'checksum': '85649ca51fd72272d7821adaf274ad91c288277713d9c18820d8499a7ff69e9a',
  'dependencies': ['typenum', 'version_check'],
  'name': 'generic-array',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.14.7'},
 {'checksum': '9229cfe53dfd69f0609a49f65461bd93001ea1ef889cd5529dd176593f5338a1',
  'dependencies': ['foldhash'],
  'name': 'hashbrown',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.15.5'},
 {'checksum': '7382cf6263419f2d8df38c55d7da83da5c18aef87fc7a7fc1fb1e344edfe14c1',
  'dependencies': ['hashbrown'],
  'name': 'hashlink',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.10.0'},
 {'checksum': '3bd1574e617a9a6c1f8f8181bd7e841e1f20e4277607ef28945765188d6284d4',
  'name': 'itoa',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.19'},
 {'dependencies': ['jojo-test-support', 'serde', 'serde_json'], 'name': 'jojo-domain', 'version': '0.1.0'},
 {'dependencies': ['jojo-domain', 'jojo-storage'], 'name': 'jojo-helper', 'version': '0.1.0'},
 {'dependencies': ['jojo-domain', 'jojo-helper', 'jojo-test-support', 'rusqlite', 'sha2'],
  'name': 'jojo-storage',
  'version': '0.1.0'},
 {'dependencies': ['jojo-domain'], 'name': 'jojo-test-support', 'version': '0.1.0'},
 {'checksum': '65cd87c9a6d60536580b316ad985726104730a5151c99845f2490428ef91240a',
  'name': 'libc',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.2.191'},
 {'checksum': '133c182a6a2c87864fe97778797e46c7e999672690dc9fa3ee8e241aa4a9c13f',
  'dependencies': ['pkg-config', 'vcpkg'],
  'name': 'libsqlite3-sys',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.35.0'},
 {'checksum': 'cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98',
  'name': 'memchr',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '2.8.3'},
 {'checksum': 'f6b464fbc74e149a392436b17d523f769e057cb6877f6a5c4618bc6f11800548',
  'name': 'pkg-config',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.3.34'},
 {'checksum': '985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9',
  'dependencies': ['unicode-ident'],
  'name': 'proc-macro2',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.107'},
 {'checksum': '1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001',
  'dependencies': ['proc-macro2'],
  'name': 'quote',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.47'},
 {'checksum': '165ca6e57b20e1351573e3729b958bc62f0e48025386970b6e4d29e7a7e71f3f',
  'dependencies': ['bitflags',
                   'fallible-iterator',
                   'fallible-streaming-iterator',
                   'hashlink',
                   'libsqlite3-sys',
                   'smallvec'],
  'name': 'rusqlite',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.37.0'},
 {'checksum': '9774ba4a74de5f7b1c1451ed6cd5285a32eddb5cccb8cc655a4e50009e06477f',
  'name': 'ryu',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.23'},
 {'checksum': '9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e',
  'dependencies': ['serde_core'],
  'name': 'serde',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.228'},
 {'checksum': '41d385c7d4ca58e59fc732af25c3983b67ac852c1a25000afe1175de458b67ad',
  'dependencies': ['serde_derive'],
  'name': 'serde_core',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.228'},
 {'checksum': 'd540f220d3187173da220f885ab66608367b6574e925011a9353e4badda91d79',
  'dependencies': ['proc-macro2', 'quote', 'syn'],
  'name': 'serde_derive',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.228'},
 {'checksum': '402a6f66d8c709116cf22f558eab210f5a50187f702eb4d7e5ef38d9a7f1c79c',
  'dependencies': ['itoa', 'memchr', 'ryu', 'serde', 'serde_core'],
  'name': 'serde_json',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.145'},
 {'checksum': 'a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283',
  'dependencies': ['cfg-if', 'cpufeatures', 'digest'],
  'name': 'sha2',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.10.9'},
 {'checksum': '149309498a5f7334b12ddae0cad8aba81ce785fe91755068867e017cf98fee46',
  'name': 'smallvec',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.16.5'},
 {'checksum': '872831b642d1a07999a962a351ed35b955ea2cfc8f3862091e2a240a84f17297',
  'dependencies': ['proc-macro2', 'quote', 'unicode-ident'],
  'name': 'syn',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '2.0.119'},
 {'checksum': 'b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20',
  'name': 'typenum',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.20.1'},
 {'checksum': 'a2c754d6c33795a1c324727428e5a7dedb5b06195f9890bdbcba760d3e246563',
  'name': 'unicode-ident',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '1.0.27'},
 {'checksum': 'accd4ea62f7bb7a82fe23066fb0957d48ef677f6eeb8215f372f52e48bb32426',
  'name': 'vcpkg',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.2.15'},
 {'checksum': '0b928f33d975fc6ad9f86c8f283853ad26bdd5b10b7f1542aa2fa15e2289105a',
  'name': 'version_check',
  'source': 'registry+https://github.com/rust-lang/crates.io-index',
  'version': '0.9.5'}]

if __name__ == "__main__":
    root = Path(__file__).resolve().parents[1]
    errors = boundary_errors(root)
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        sys.exit(1)
    print("Boundary PASS: approved pinned dependencies; no network/auth adapters; Codex adapter is offline translation only; "
          "no workspace unsafe/FFI; process launches confined to self/helper fixtures, the local Git boundary "
          "and the P1-09 supervised launch surface.")
