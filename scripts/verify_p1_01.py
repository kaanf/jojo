#!/usr/bin/env python3
"""T01: clean-home/target, cached-dependency, network-denied local builds and meaningful contract checks.

Requires preinstalled macOS toolchains. Never installs/downloads anything or launches a provider.
Run from any directory: python3 /absolute/path/to/scripts/verify_p1_01.py

Since P1-02 this file is also the shared runner: `verify_p1_02.py` calls `main` with its
own task name and extra checks, so the T01 regression is the same code path. Run directly,
it performs the T01 regression against the current sources and writes
`verification/p1-01-regression-result.json`; the original `p1-01-result.json` is kept as
the historical record of the P1-01 source state and is never overwritten.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "verification"
SANDBOX = "(version 1) (allow default) (deny network*)"


def main(task="P1-01-regression", scope="T01 regression on the current workspace (P1-01 checks only)", extra=None):
    stem = task.lower()
    tools = {name: shutil.which(name) for name in ["cargo", "rustc", "swift", "git", "sandbox-exec"]}
    if sys.platform != "darwin" or not all(tools.values()):
        raise RuntimeError("T01 requires preinstalled macOS Rust/Swift/Git and sandbox-exec; no fallback/download")
    records = []
    log = []
    with tempfile.TemporaryDirectory(prefix="jojo-t01-") as temporary:
        temp = Path(temporary)
        for name in ["home", "cargo", "tmp", "cache", "clang-cache", "swift-cache"]:
            (temp / name).mkdir(mode=0o700)
        # Reuse only registry artifacts; never inherit Cargo configuration or credentials.
        cache_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        registry = cache_home / "registry"
        if not registry.is_dir():
            raise RuntimeError("Cache dependencies first: cargo fetch --locked --config net.offline=false")
        (temp / "cargo/registry").symlink_to(registry.resolve(), target_is_directory=True)
        env = {
            "HOME": str(temp / "home"),
            "CARGO_HOME": str(temp / "cargo"),
            "CARGO_TARGET_DIR": str(temp / "target"),
            "CARGO_NET_OFFLINE": "true",
            "RUSTUP_AUTO_INSTALL": "0",
            "RUSTC": tools["rustc"],
            "PATH": os.pathsep.join(dict.fromkeys([str(Path(tools["rustc"]).parent), "/usr/bin", "/bin", "/usr/sbin", "/sbin"])),
            "TMPDIR": str(temp / "tmp"),
            "XDG_CACHE_HOME": str(temp / "cache"),
            "CLANG_MODULE_CACHE_PATH": str(temp / "clang-cache"),
            "SWIFTPM_MODULECACHE_OVERRIDE": str(temp / "swift-cache"),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_OPTIONAL_LOCKS": "0",
            "PYTHONDONTWRITEBYTECODE": "1",
            "LANG": "en_US.UTF-8",
        }

        def run(label, argv, expected=0, cwd=ROOT):
            completed = subprocess.run([tools["sandbox-exec"], "-p", SANDBOX, *map(str, argv)], cwd=cwd, env=env, text=True, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=240)
            print(f"{label}: exit {completed.returncode}", flush=True)
            combined = completed.stdout + completed.stderr
            log.append(f"$ {label}\n{combined}\n")
            records.append({"check": label, "exit_code": completed.returncode, "expected_exit_code": expected})
            if completed.returncode != expected:
                raise RuntimeError(f"{label}\n{combined}")
            run.last_output = combined
            return completed.stdout

        probe = 'import errno,socket; s=socket.socket(); result=s.connect_ex(("127.0.0.1",9)); s.close(); assert result in (errno.EPERM,errno.EACCES), result; print("network denied by OS sandbox")'
        # A denied loopback connect sends no API request or external traffic.
        run("OS network-denial probe", [sys.executable, "-c", probe])
        run("static architecture boundary", [sys.executable, ROOT / "scripts/check_p1_01_boundaries.py"])
        run("boundary regression tests", [sys.executable, "-m", "unittest", "discover", "-s", ROOT / "scripts/tests", "-v"])
        rust_version = run("Rust compiler identity", [tools["rustc"], "--version"]).strip()
        if not rust_version.startswith("rustc 1.95.0 "):
            raise RuntimeError("Pinned Rust 1.95.0 must be installed before T01; no automatic installation")
        cargo_version = run("Cargo identity", [tools["cargo"], "--version"]).strip()
        swift_version = run("Swift identity", [tools["swift"], "--version"]).strip()
        metadata = json.loads(run("offline locked metadata", [tools["cargo"], "metadata", "--offline", "--locked", "--format-version", "1"]))
        spec = importlib.util.spec_from_file_location("boundaries", ROOT / "scripts/check_p1_01_boundaries.py")
        boundary = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(boundary)
        expected = {(p["name"], p["version"], p.get("source")) for p in boundary.APPROVED_PACKAGES}
        if {(p["name"], p["version"], p["source"]) for p in metadata["packages"]} != expected:
            raise RuntimeError("Resolved packages differ from the approved dependency graph")
        helper = next(p for p in metadata["packages"] if p["name"] == "jojo-helper")
        if any(d["name"] == "jojo-test-support" and d["kind"] is None for d in helper["dependencies"]):
            raise RuntimeError("Test fixtures cannot be production helper dependencies")
        run("Rust formatting", [tools["cargo"], "fmt", "--all", "--", "--check"])
        run("Rust targets compile", [tools["cargo"], "build", "--offline", "--locked", "--workspace", "--all-targets"])
        run("Rust deterministic contract tests", [tools["cargo"], "test", "--offline", "--locked", "--workspace", "--all-targets"])
        test_output = run.last_output
        run("Rust documentation tests", [tools["cargo"], "test", "--offline", "--locked", "--workspace", "--doc"])
        run("Rust Clippy", [tools["cargo"], "clippy", "--offline", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"])
        binary = temp / "target/debug/jojo-helper"
        run("helper default test-home roots", [binary, "--check-config"])
        run("helper explicit roots", [binary, "--check-config", "--data-root", temp / "explicit-data", "--worktree-root", temp / "explicit-worktrees"])
        run("helper rejects execution command", [binary, "--run"], expected=2)
        run("helper has no implicit runtime", [binary], expected=2)
        if any(p.exists() for p in [temp / "explicit-data", temp / "explicit-worktrees", temp / "home/Library/Application Support/Jojo", temp / "home/Library/Application Support/JojoWorktrees"]):
            raise RuntimeError("--check-config unexpectedly created runtime directories")
        package = ROOT / "macos/JojoApp"
        common = ["--package-path", package, "--scratch-path", temp / "swift-build", "--cache-path", temp / "swiftpm-cache", "--disable-sandbox"]
        manifest = json.loads(run("Swift package manifest", [tools["swift"], "package", *common, "dump-package"]))
        if manifest["name"] != "JojoAppBoundary" or manifest["dependencies"]:
            raise RuntimeError("Swift boundary must be a dependency-free native package")
        run("SwiftUI boundary compiles", [tools["swift"], "build", *common, "--disable-automatic-resolution"])
        baseline = json.loads((EVIDENCE / "p1-01-preservation-baseline.json").read_text())
        for relative, digest in baseline["documents"].items():
            if hashlib.sha256((ROOT / relative).read_bytes()).hexdigest() != digest:
                raise RuntimeError(f"Historical document modified: {relative}")
        for clone in baseline["clones"]:
            directory = ROOT / "research/upstream" / clone["name"]
            head = run(f"preserved {clone['name']} pin", [tools["git"], "-C", directory, "rev-parse", "HEAD"]).strip()
            status = run(f"preserved {clone['name']} checkout", [tools["git"], "-C", directory, "status", "--porcelain", "--untracked-files=all"]).strip()
            if head != clone["head"] or status != clone["status"]:
                raise RuntimeError(f"Research checkout changed: {clone['name']}")
        additions = extra(run, tools, temp, test_output) if extra else {}
        result = {
            "task": task, "passed": True,
            "environment": {"credentials": "none inherited; temporary HOME/CARGO_HOME", "network": "OS sandbox denied; loopback probe confirmed", "external_packages": sum(p["source"] is not None for p in metadata["packages"]), "dependency_cache": "registry only; offline and locked"},
            "toolchains": {"rustc": rust_version, "cargo": cargo_version, "swift": swift_version},
            "checks": records, "preserved_documents": len(baseline["documents"]), "preserved_clones": len(baseline["clones"]),
            "scope": scope, **additions,
            "input_sha256": {
                str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in sorted([
                    ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "rust-toolchain.toml",
                    ROOT / ".cargo/config.toml", EVIDENCE / "p1-01-preservation-baseline.json",
                    *[p for p in (ROOT / "crates").rglob("*") if p.is_file() and p.suffix in {".rs", ".toml"}],
                    *[p for p in (ROOT / "macos").rglob("*.swift")],
                    *[p for p in (ROOT / "scripts").rglob("*.py")],
                ])
            },
        }
        (EVIDENCE / f"{stem}-result.json").write_text(json.dumps(result, indent=2) + "\n")
        (EVIDENCE / f"{stem}.log").write_text("\n".join(log))
        print(f"{task} PASS; machine evidence and tool output saved in verification/.")


def guarded(task, **arguments):
    """Runs `main`, replacing any earlier pass record with the failure if a check fails."""
    target = EVIDENCE / f"{task.lower()}-result.json"
    target.write_text(json.dumps({"task": task, "passed": False, "status": "running"}) + "\n")
    try:
        main(task, **arguments)
    except (RuntimeError, AssertionError, subprocess.SubprocessError) as error:
        target.write_text(json.dumps({"task": task, "passed": False, "status": "failed", "error": str(error)}, indent=2) + "\n")
        print(str(error), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    guarded("P1-01-regression")
