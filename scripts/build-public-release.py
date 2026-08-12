#!/usr/bin/env python3
"""Build a host release binary without embedding maintainer filesystem paths."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile


PUBLIC_ALIASES = {
    "home": "/redacted/home",
    "cargo": "/redacted/cargo",
    "rustup": "/redacted/rustup",
    "temp": "/redacted/tmp",
    "workspace": "/src/maple-agent-market",
}


def remap_flags(paths: dict[str, pathlib.Path]) -> list[str]:
    """Return stable rustc path remaps, with more-specific prefixes last."""

    entries: list[tuple[int, str, str]] = []
    for label, path in paths.items():
        raw_spellings = {str(path), path.as_posix()}
        for spelling in raw_spellings:
            normalized = spelling.rstrip("/\\")
            if len(normalized) < 4:
                continue
            entries.append((len(normalized), normalized, PUBLIC_ALIASES[label]))
    entries.sort(key=lambda item: (item[0], item[1], item[2]))
    return [f"--remap-path-prefix={source}={alias}" for _, source, alias in entries]


def git_root(start: pathlib.Path | None = None) -> pathlib.Path:
    """Resolve the worktree containing this script, independent of caller cwd."""

    output = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        cwd=start or pathlib.Path(__file__).resolve().parent,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    ).stdout.strip()
    return pathlib.Path(output).resolve()


def release_target_dir(root: pathlib.Path) -> pathlib.Path:
    """Return a deterministic absolute Cargo target directory."""

    configured = os.environ.get("CARGO_TARGET_DIR")
    target_dir = pathlib.Path(configured) if configured else root / "target"
    if not target_dir.is_absolute():
        target_dir = root / target_dir
    return target_dir.resolve()


def environment_path(root: pathlib.Path, name: str, default: pathlib.Path) -> pathlib.Path:
    """Resolve a path-valued environment variable as Cargo sees it from root."""

    path = pathlib.Path(os.environ.get(name, default))
    if not path.is_absolute():
        path = root / path
    return path.resolve()


def release_artifact(root: pathlib.Path, target: str | None = None) -> pathlib.Path:
    """Return the release executable path for an explicit or configured target."""

    target_dir = release_target_dir(root)
    target = target or os.environ.get("CARGO_BUILD_TARGET")
    suffix = ".exe" if os.name == "nt" else ""
    target_parts = [target] if target else []
    return target_dir.joinpath(*target_parts, "release", f"pixtuoid{suffix}")


def rustc_host(root: pathlib.Path) -> str:
    """Read the host target used for this host-only release build."""

    output = subprocess.run(
        [os.environ.get("RUSTC", "rustc"), "-vV"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    ).stdout
    for line in output.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ").strip()
    raise ValueError("rustc -vV did not report a host target")


def public_paths(root: pathlib.Path) -> dict[str, pathlib.Path]:
    """Collect private build prefixes that rustc must rewrite."""

    home = pathlib.Path.home().resolve()
    return {
        "home": home,
        "cargo": environment_path(root, "CARGO_HOME", home / ".cargo"),
        "rustup": environment_path(root, "RUSTUP_HOME", home / ".rustup"),
        "temp": pathlib.Path(tempfile.gettempdir()).resolve(),
        "workspace": root,
    }


def run_audit(root: pathlib.Path, artifact: pathlib.Path | None = None) -> None:
    """Run the source gate, optionally including one built artifact."""

    audit = root / "scripts" / "public-release-audit.py"
    command = [sys.executable, str(audit)]
    if artifact is not None:
        command.extend(("--artifact", str(artifact)))
    subprocess.run(command, cwd=root, check=True)


def build_release(root: pathlib.Path) -> pathlib.Path:
    """Build and return the exact host executable through an explicit target path."""

    flags = remap_flags(public_paths(root))
    cargo_config = f"target.'cfg(all())'.rustflags = {json.dumps(flags)}"
    target = rustc_host(root)
    target_dir = release_target_dir(root)
    subprocess.run(
        [
            os.environ.get("CARGO", "cargo"),
            "--config",
            cargo_config,
            "build",
            "--locked",
            "--release",
            "--target",
            target,
            "--target-dir",
            str(target_dir),
            "-p",
            "pixtuoid",
            "--bin",
            "pixtuoid",
        ],
        cwd=root,
        check=True,
    )
    return release_artifact(root, target)


def selftest() -> int:
    """Pin path spelling, specificity ordering and stable public aliases."""

    paths = {
        "home": pathlib.Path("C:/Users/Test Owner"),
        "cargo": pathlib.Path("C:/Users/Test Owner/.cargo"),
        "workspace": pathlib.Path("C:/Users/Test Owner/work/maple-agent-market"),
    }
    flags = remap_flags(paths)
    expected = {
        "--remap-path-prefix=C:/Users/Test Owner=/redacted/home",
        "--remap-path-prefix=C:/Users/Test Owner/.cargo=/redacted/cargo",
        "--remap-path-prefix=C:/Users/Test Owner/work/maple-agent-market=/src/maple-agent-market",
    }
    if not expected.issubset(set(flags)):
        print(f"FAIL: remap flags missing expected entries: {sorted(expected - set(flags))}")
        return 1
    home_index = flags.index("--remap-path-prefix=C:/Users/Test Owner=/redacted/home")
    workspace_index = flags.index(
        "--remap-path-prefix=C:/Users/Test Owner/work/maple-agent-market=/src/maple-agent-market"
    )
    if home_index >= workspace_index:
        print("FAIL: a broad home remap must precede the more-specific workspace remap")
        return 1
    target = "x86_64-pc-windows-msvc"
    artifact = release_artifact(pathlib.Path.cwd(), target)
    if target not in artifact.parts:
        print("FAIL: release artifact path omitted the explicit Cargo target")
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        unicode_root = pathlib.Path(tmp) / "繁體中文-repo"
        unicode_root.mkdir()
        subprocess.run(["git", "init", "--quiet"], cwd=unicode_root, check=True)
        if git_root(unicode_root) != unicode_root.resolve():
            print("FAIL: git root must decode a UTF-8 non-ASCII worktree path")
            return 1
    print("build-public-release selftest passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()
    if args.selftest:
        return selftest()

    if os.environ.get("RUSTFLAGS") or os.environ.get("CARGO_ENCODED_RUSTFLAGS"):
        print(
            "public release build refuses inherited RUSTFLAGS; unset them so "
            "the audited path-remap configuration cannot be bypassed",
            file=sys.stderr,
        )
        return 2

    root = git_root()
    subprocess.run(
        [sys.executable, str(root / "scripts" / "public-release-audit.py"), "--selftest"],
        cwd=root,
        check=True,
    )
    run_audit(root)
    artifact = build_release(root)
    run_audit(root, artifact)
    print(f"public release build passed: {artifact}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
