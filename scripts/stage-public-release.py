#!/usr/bin/env python3
"""Stage a public-safe binary bundle after every release gate has passed."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import tomllib


SOURCE_BUNDLE_FILES = (
    ("LICENSE", "LICENSE", "MIT"),
    ("README.md", "README.md", "MIT"),
    ("FORK_NOTICE.md", "FORK_NOTICE.md", "MIT"),
    ("THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md", "MIT"),
    ("docs/OPEN_SOURCE_RELEASE.md", "OPEN_SOURCE_RELEASE.md", "MIT"),
    ("crates/pixtuoid/fonts/OFL-Monaspace.txt", "OFL-Monaspace.txt", "OFL-1.1"),
)


@dataclass(frozen=True)
class BundleRequest:
    """All immutable inputs required to stage one public bundle."""

    root: pathlib.Path
    artifact: pathlib.Path
    output: pathlib.Path
    version: str
    revision: str


def sha256(path: pathlib.Path) -> str:
    """Return a lowercase SHA-256 for one staged file."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalized_request(request: BundleRequest) -> BundleRequest:
    """Resolve filesystem inputs before applying containment checks."""

    return BundleRequest(
        root=request.root.resolve(),
        artifact=request.artifact.resolve(),
        output=request.output.resolve(),
        version=request.version,
        revision=request.revision,
    )


def validate_request(request: BundleRequest) -> list[tuple[pathlib.Path, str, str]]:
    """Validate destinations and return the exact source-file inventory."""

    if request.output.exists():
        raise FileExistsError(
            f"refusing to overwrite existing output: {request.output}"
        )
    try:
        request.output.relative_to(request.root)
    except ValueError:
        pass
    else:
        raise ValueError("public bundle output must be outside the source repository")
    if not request.artifact.is_file():
        raise FileNotFoundError(f"release artifact is missing: {request.artifact}")

    source_entries: list[tuple[pathlib.Path, str, str]] = []
    for source_name, staged_name, licence in SOURCE_BUNDLE_FILES:
        source = request.root / pathlib.PurePosixPath(source_name)
        if not source.is_file():
            raise FileNotFoundError(f"required bundle source is missing: {source_name}")
        source_entries.append((source, staged_name, licence))
    return source_entries


def copy_source_components(
    source_entries: list[tuple[pathlib.Path, str, str]], staging: pathlib.Path
) -> list[dict[str, str]]:
    """Copy notices and licences and return their manifest entries."""

    components: list[dict[str, str]] = []
    for source, staged_name, licence in source_entries:
        destination = staging / staged_name
        shutil.copy2(source, destination)
        components.append(
            {"path": staged_name, "sha256": sha256(destination), "spdx": licence}
        )
    return components


def copy_executable(
    artifact: pathlib.Path, staging: pathlib.Path
) -> tuple[str, dict[str, str]]:
    """Copy the release executable and return its name and manifest entry."""

    executable_name = "pixtuoid.exe" if artifact.suffix.casefold() == ".exe" else "pixtuoid"
    executable = staging / executable_name
    shutil.copy2(artifact, executable)
    return executable_name, {
        "path": executable_name,
        "sha256": sha256(executable),
        "spdx": "MIT AND OFL-1.1",
    }


def write_bundle_manifest(
    staging: pathlib.Path,
    request: BundleRequest,
    executable_name: str,
    components: list[dict[str, str]],
) -> None:
    """Write the machine-readable public/private asset boundary."""

    manifest = {
        "schema_version": 1,
        "project": "Maple Agent Market",
        "version": request.version,
        "source_revision": request.revision,
        "profile": "public-safe",
        "entrypoint": executable_name,
        "contains_private_maple_assets": False,
        "excluded_asset_classes": [
            "NEXON or MapleStory images, sprites, maps, monsters, portals, and skill frames",
            "MapleStory music or downloaded streaming audio",
            "Open API paperdolls and generated local skins",
            "local previews, QA captures, caches, and active packs",
        ],
        "components": sorted(components, key=lambda item: item["path"]),
    }
    (staging / "PUBLIC_BUNDLE_MANIFEST.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def write_checksum_inventory(staging: pathlib.Path) -> None:
    """Write SHA-256 entries for every staged file except the inventory itself."""

    checksum_paths = sorted(
        (path for path in staging.iterdir() if path.name != "SHA256SUMS.txt"),
        key=lambda path: path.name,
    )
    checksum_text = "".join(
        f"{sha256(path)}  {path.name}\n" for path in checksum_paths
    )
    (staging / "SHA256SUMS.txt").write_text(
        checksum_text,
        encoding="utf-8",
        newline="\n",
    )


def stage_bundle(request: BundleRequest) -> pathlib.Path:
    """Copy the exact public-safe binary bundle without overwriting anything."""

    request = normalized_request(request)
    source_entries = validate_request(request)

    request.output.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(
        tempfile.mkdtemp(
            prefix=f".{request.output.name}-staging-", dir=request.output.parent
        )
    )
    try:
        components = copy_source_components(source_entries, staging)
        executable_name, executable_component = copy_executable(
            request.artifact, staging
        )
        components.append(executable_component)
        write_bundle_manifest(staging, request, executable_name, components)
        write_checksum_inventory(staging)
        os.replace(staging, request.output)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    return request.output


def git_root() -> pathlib.Path:
    """Resolve the repository containing this script."""

    output = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        cwd=pathlib.Path(__file__).resolve().parent,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    return pathlib.Path(output).resolve()


def require_clean_publication_candidate(root: pathlib.Path) -> None:
    """Refuse a bundle whose source revision cannot reproduce the working tree."""

    status = subprocess.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    if status.strip():
        count = len(status.splitlines())
        raise RuntimeError(
            f"publication candidate is dirty ({count} status entries); commit or remove "
            "the reviewed candidate before staging"
        )


def require_fork_metadata(root: pathlib.Path) -> str:
    """Return the version only after fork-owned release links are configured."""

    document = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    package = document.get("workspace", {}).get("package", {})
    version = package.get("version")
    if not isinstance(version, str) or not version:
        raise RuntimeError("workspace package version is missing")
    upstream = "https://github.com/IvanWng97/pixtuoid"
    repository = package.get("repository")
    homepage = package.get("homepage")
    unresolved: list[str] = []
    if (
        not isinstance(repository, str)
        or not repository.startswith("https://github.com/")
        or repository.rstrip("/") == upstream
    ):
        unresolved.append("repository")
    if (
        not isinstance(homepage, str)
        or not homepage.startswith("https://")
        or homepage.rstrip("/") == upstream
    ):
        unresolved.append("homepage")
    if unresolved:
        raise RuntimeError(
            "fork publication metadata is unresolved: " + ", ".join(unresolved)
        )
    return version


def built_release_artifact(root: pathlib.Path) -> pathlib.Path:
    """Resolve the host-target artifact produced by build-public-release.py."""

    rustc = os.environ.get("RUSTC", "rustc")
    version_output = subprocess.run(
        [rustc, "-vV"], cwd=root, check=True, capture_output=True, text=True
    ).stdout
    host = next(
        (line.removeprefix("host: ").strip() for line in version_output.splitlines() if line.startswith("host: ")),
        None,
    )
    if not host:
        raise RuntimeError("rustc -vV did not report a host target")
    configured = pathlib.Path(os.environ.get("CARGO_TARGET_DIR", root / "target"))
    target_dir = configured if configured.is_absolute() else root / configured
    suffix = ".exe" if os.name == "nt" else ""
    return target_dir.resolve() / host / "release" / f"pixtuoid{suffix}"


def run_release_build(root: pathlib.Path) -> pathlib.Path:
    """Run the source and binary gates, then return the audited executable."""

    subprocess.run(
        [sys.executable, str(root / "scripts" / "public-release-audit.py"), "--selftest"],
        cwd=root,
        check=True,
    )
    subprocess.run(
        [sys.executable, str(root / "scripts" / "stage-public-release.py"), "--selftest"],
        cwd=root,
        check=True,
    )
    subprocess.run(
        [sys.executable, str(root / "scripts" / "build-public-release.py")],
        cwd=root,
        check=True,
    )
    return built_release_artifact(root)


def selftest() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp) / "source"
        artifact = pathlib.Path(tmp) / "build" / "pixtuoid.exe"
        output = pathlib.Path(tmp) / "public-bundle"
        source_files = {
            "Cargo.toml": """
[workspace.package]
version = "0.16.0"
repository = "https://github.com/IvanWng97/pixtuoid"
homepage = "https://github.com/IvanWng97/pixtuoid"
""".lstrip(),
            "LICENSE": "MIT fixture\n",
            "README.md": "# Fixture\n",
            "FORK_NOTICE.md": "unofficial fixture\n",
            "THIRD_PARTY_NOTICES.md": "notices fixture\n",
            "docs/OPEN_SOURCE_RELEASE.md": "release fixture\n",
            "crates/pixtuoid/fonts/OFL-Monaspace.txt": "OFL fixture\n",
        }
        for relative, content in source_files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        artifact.parent.mkdir(parents=True, exist_ok=True)
        artifact.write_bytes(b"MZ public fixture")

        try:
            require_fork_metadata(root)
        except RuntimeError:
            pass
        else:
            failures.append("upstream repository metadata must block public staging")
        (root / "Cargo.toml").write_text(
            """
[workspace.package]
version = "0.16.0"
repository = "https://github.com/example/maple-agent-market"
homepage = "https://example.github.io/maple-agent-market"
""".lstrip(),
            encoding="utf-8",
        )
        if require_fork_metadata(root) != "0.16.0":
            failures.append("resolved fork metadata must preserve the workspace version")

        request = BundleRequest(root, artifact, output, "0.16.0", "abc123")
        stage_bundle(request)

        expected = {
            "FORK_NOTICE.md",
            "LICENSE",
            "OFL-Monaspace.txt",
            "OPEN_SOURCE_RELEASE.md",
            "PUBLIC_BUNDLE_MANIFEST.json",
            "README.md",
            "SHA256SUMS.txt",
            "THIRD_PARTY_NOTICES.md",
            "pixtuoid.exe",
        }
        observed = {path.name for path in output.iterdir()}
        if observed != expected:
            failures.append(f"bundle mismatch: expected {expected}, observed {observed}")

        manifest = json.loads((output / "PUBLIC_BUNDLE_MANIFEST.json").read_text("utf-8"))
        if manifest.get("profile") != "public-safe" or manifest.get(
            "contains_private_maple_assets"
        ) is not False:
            failures.append("manifest does not pin the public-safe/private-asset boundary")

        checksum_lines = (output / "SHA256SUMS.txt").read_text("utf-8").splitlines()
        checksum_names = {line.split("  ", 1)[1] for line in checksum_lines}
        if checksum_names != expected - {"SHA256SUMS.txt"}:
            failures.append("checksum inventory is not the exact staged bundle")

        try:
            stage_bundle(request)
        except FileExistsError:
            pass
        else:
            failures.append("an existing output directory must never be overwritten")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1
    print("stage-public-release selftest passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument(
        "--output",
        type=pathlib.Path,
        help="new directory outside the repository; existing paths are refused",
    )
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    if args.output is None:
        parser.error("--output is required")

    try:
        root = git_root()
        require_clean_publication_candidate(root)
        version = require_fork_metadata(root)
        artifact = run_release_build(root)
        revision = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        output = stage_bundle(
            BundleRequest(
                root=root,
                artifact=artifact,
                output=args.output,
                version=version,
                revision=revision,
            )
        )
    except (FileExistsError, FileNotFoundError, OSError, RuntimeError, ValueError) as error:
        print(f"public release staging refused: {error}", file=sys.stderr)
        return 2
    print(f"public-safe bundle staged: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
