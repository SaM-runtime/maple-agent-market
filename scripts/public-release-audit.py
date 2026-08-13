#!/usr/bin/env python3
"""Reject private or unapproved files before a public source release.

Usage: public-release-audit.py [--selftest] [--artifact PATH]

The live audit examines the exact Git publication candidates: tracked files plus
untracked files that are not ignored. Optional artifacts are checked for
embedded current-machine paths. Findings print only a stable rule id and
path/line; matched private values are never echoed.
"""

from __future__ import annotations

import argparse
import dataclasses
import fnmatch
import hashlib
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib


@dataclasses.dataclass(frozen=True)
class Finding:
    """One sanitized release-readiness failure."""

    rule: str
    path: str
    line: int | None = None


@dataclasses.dataclass(frozen=True)
class MediaLicenceRule:
    """One reviewed provenance group for hash-approved repository media."""

    rule_id: str
    spdx: str
    patterns: tuple[str, ...]
    source_url: str = ""
    licence_file: str = ""
    notice_file: str = ""


@dataclasses.dataclass(frozen=True)
class PublicationPolicy:
    """Reviewed media and workstation-path rules for one audit run."""

    media_allowlist: dict[str, str]
    media_licence_rules: tuple[MediaLicenceRule, ...]
    private_tokens: tuple[str, ...] = ()


MEDIA_SUFFIXES = frozenset(
    {
        ".avi",
        ".avif",
        ".bmp",
        ".gif",
        ".ico",
        ".jpeg",
        ".jpg",
        ".m4v",
        ".mkv",
        ".mov",
        ".mp4",
        ".otf",
        ".pdf",
        ".png",
        ".sprite",
        ".svg",
        ".ttf",
        ".wasm",
        ".webp",
        ".webm",
        ".woff",
        ".woff2",
    }
)
FORBIDDEN_BINARY_SUFFIXES = frozenset(
    {
        ".7z",
        ".a",
        ".aac",
        ".appx",
        ".bz2",
        ".cab",
        ".crate",
        ".deb",
        ".dll",
        ".dmg",
        ".dylib",
        ".exe",
        ".flac",
        ".gz",
        ".lib",
        ".m4a",
        ".mp3",
        ".msi",
        ".msix",
        ".o",
        ".obj",
        ".ogg",
        ".pdb",
        ".pkg",
        ".rar",
        ".rpm",
        ".so",
        ".tar",
        ".tgz",
        ".wav",
        ".xz",
        ".zip",
        ".zst",
    }
)
FORBIDDEN_BINARY_MAGICS = (
    b"7z\xbc\xaf'\x1c",
    b"BZh",
    b"GIF87a",
    b"GIF89a",
    b"MZ",
    b"PK\x03\x04",
    b"PK\x05\x06",
    b"PK\x07\x08",
    b"Rar!\x1a\x07",
    b"\x1f\x8b",
    b"\x7fELF",
    b"\x89PNG\r\n\x1a\n",
    b"\xff\xd8\xff",
)
PRIVATE_SEGMENTS = frozenset({".codex", "private-assets", "pixtuoid-maple-assets"})
APPROVED_PROJECT_CONFIGS = frozenset({".codex/config.toml"})
PRIVATE_PREFIXES = (
    "artifacts/",
    "generated/maplestory",
    "generated/nexon",
    "sources/maplestory",
    "sources/nexon",
)
PRIVATE_SPRITE_NAMES = frozenset({"scene_background.sprite", "training_background.sprite"})
PRIVATE_SPRITE_PREFIXES = (
    "market_avatar_",
    "training_avatar_",
    "training_monster_",
    "training_portal_",
    "training_skill_",
)
CREDENTIAL_PATTERNS = tuple(
    re.compile(pattern)
    for pattern in (
        rb"AKIA[0-9A-Z]{16}",
        rb"gh[pousr]_[A-Za-z0-9_]{20,}",
        rb"github_pat_[A-Za-z0-9_]{20,}",
        rb"sk-(?:ant-)?[A-Za-z0-9_-]{20,}",
        rb"-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----",
    )
)
UPSTREAM_FUNDING_NEEDLES = (b"buymeacoffee.com/" + b"IvanWng97",)


def normalize_path(path: str) -> str:
    """Return one repository-relative path spelling for comparisons."""

    normalized = pathlib.PurePosixPath(path.replace("\\", "/")).as_posix()
    return normalized.removeprefix("./")


def is_private_asset_path(path: str) -> bool:
    """Whether a candidate path belongs to the local Maple/private boundary."""

    normalized = normalize_path(path)
    lowered = normalized.casefold()
    if lowered in APPROVED_PROJECT_CONFIGS:
        return False
    parts = {part.casefold() for part in pathlib.PurePosixPath(normalized).parts}
    if parts & PRIVATE_SEGMENTS:
        return True
    if lowered.startswith(PRIVATE_PREFIXES):
        return True
    name = pathlib.PurePosixPath(normalized).name.casefold()
    return name in PRIVATE_SPRITE_NAMES or (
        name.endswith(".sprite") and name.startswith(PRIVATE_SPRITE_PREFIXES)
    )


def first_matching_line(data: bytes, needles: tuple[bytes, ...]) -> int | None:
    """Return the first one-based line containing a private byte token."""

    folded_needles = tuple(needle.lower() for needle in needles if needle)
    for number, line in enumerate(data.splitlines(), start=1):
        folded = line.lower()
        if any(needle in folded for needle in folded_needles):
            return number
    return None


def first_credential_line(data: bytes) -> int | None:
    """Return the first one-based line matching a high-confidence credential."""

    for number, line in enumerate(data.splitlines(), start=1):
        if any(pattern.search(line) for pattern in CREDENTIAL_PATTERNS):
            return number
    return None


def has_forbidden_binary_magic(data: bytes) -> bool:
    """Whether an extensionless/disguised candidate starts as known binary data."""

    return data.startswith(FORBIDDEN_BINARY_MAGICS)


def candidate_exists(path: pathlib.Path) -> bool:
    """Whether a publication candidate exists, including a broken symlink."""

    return path.is_file() or path.is_symlink()


def read_candidate_bytes(path: pathlib.Path) -> bytes:
    """Read the bytes Git would publish, rather than following a symlink target."""

    if path.is_symlink():
        return os.readlink(path).encode("utf-8", errors="surrogateescape")
    return path.read_bytes()


def artifact_contains_private_token(data: bytes, private_tokens: tuple[str, ...]) -> bool:
    """Whether a binary artifact embeds a current-machine path token."""

    for token in private_tokens:
        if not token:
            continue
        spellings = {token, token.replace("\\", "/"), token.replace("/", "\\")}
        for spelling in spellings:
            if not spelling:
                continue
            if spelling.encode("utf-8") in data or spelling.encode("utf-16-le") in data:
                return True
    return False


def audit_artifacts(
    root: pathlib.Path,
    artifacts: list[pathlib.Path],
    private_tokens: tuple[str, ...],
) -> list[Finding]:
    """Reject missing or current-machine-path-bearing release artifacts."""

    findings: list[Finding] = []
    for requested in artifacts:
        artifact = requested if requested.is_absolute() else root / requested
        artifact = artifact.resolve()
        try:
            label = normalize_path(str(artifact.relative_to(root)))
        except ValueError:
            label = artifact.name
        if not artifact.is_file():
            findings.append(Finding("artifact-missing", label))
            continue
        try:
            data = artifact.read_bytes()
        except OSError:
            findings.append(Finding("artifact-unreadable", label))
            continue
        if artifact_contains_private_token(data, private_tokens):
            findings.append(Finding("artifact-private-path", label))
    return findings


def audit_media_licence_coverage(
    media_paths: list[str],
    rules: tuple[MediaLicenceRule, ...],
) -> list[Finding]:
    """Require every approved media path to match exactly one provenance rule."""

    findings: list[Finding] = []
    for path in sorted({normalize_path(path) for path in media_paths}):
        matched = [
            rule.rule_id
            for rule in rules
            if any(fnmatch.fnmatchcase(path, pattern) for pattern in rule.patterns)
        ]
        if not matched:
            findings.append(Finding("media-licence-missing", path))
        elif len(matched) > 1:
            findings.append(Finding("media-licence-ambiguous", path))
    return findings


def audit_media_licence_evidence(
    candidate_paths: set[str],
    rules: tuple[MediaLicenceRule, ...],
) -> list[Finding]:
    """Require each declared licence and notice file to ship with the source."""

    findings: list[Finding] = []
    evidence = {
        normalize_path(path)
        for rule in rules
        for path in (rule.licence_file, rule.notice_file)
        if path
    }
    for path in sorted(evidence - candidate_paths):
        findings.append(Finding("media-licence-evidence-missing", path))
    return findings


def audit_candidates(
    root: pathlib.Path,
    paths: list[str],
    policy: PublicationPolicy,
) -> list[Finding]:
    """Return sanitized findings for candidate repository paths."""

    findings: list[Finding] = []
    normalized_paths = sorted({normalize_path(path) for path in paths})
    candidate_set = set(normalized_paths)
    private_needles: list[bytes] = []
    approved_media: list[str] = []
    for token in policy.private_tokens:
        if not token:
            continue
        for spelling in {token, token.replace("\\", "/"), token.replace("/", "\\")}:  # noqa: E501
            private_needles.append(spelling.encode("utf-8"))

    for path in normalized_paths:
        file_path = root / pathlib.PurePosixPath(path)
        if not candidate_exists(file_path):
            findings.append(Finding("candidate-missing", path))
            continue
        if is_private_asset_path(path):
            findings.append(Finding("private-asset-path", path))
            continue

        suffix = file_path.suffix.casefold()
        if suffix in FORBIDDEN_BINARY_SUFFIXES:
            findings.append(Finding("forbidden-binary-media", path))
            continue
        data = read_candidate_bytes(file_path)
        if suffix in MEDIA_SUFFIXES:
            expected_hash = policy.media_allowlist.get(path)
            if expected_hash is None:
                findings.append(Finding("media-not-approved", path))
                continue
            actual_hash = hashlib.sha256(data).hexdigest()
            if actual_hash != expected_hash:
                findings.append(Finding("media-hash-mismatch", path))
                continue
            approved_media.append(path)

        if suffix not in MEDIA_SUFFIXES and (
            b"\0" in data[:8192] or has_forbidden_binary_magic(data)
        ):
            findings.append(Finding("binary-not-approved", path))
            continue
        if line := first_matching_line(data, tuple(private_needles)):
            findings.append(Finding("private-local-path", path, line))
        if line := first_matching_line(data, UPSTREAM_FUNDING_NEEDLES):
            findings.append(Finding("upstream-funding-link", path, line))
        if line := first_credential_line(data):
            findings.append(Finding("credential-pattern", path, line))

    for path in sorted(set(policy.media_allowlist) - candidate_set):
        findings.append(Finding("media-allowlist-stale", path))
    findings.extend(
        audit_media_licence_coverage(approved_media, policy.media_licence_rules)
    )
    findings.extend(
        audit_media_licence_evidence(candidate_set, policy.media_licence_rules)
    )
    return findings


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


def git_candidates(root: pathlib.Path) -> list[str]:
    """Tracked plus non-ignored untracked files: everything commit could publish."""

    output = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root,
        check=True,
        capture_output=True,
    ).stdout
    paths = [normalize_path(raw.decode("utf-8")) for raw in output.split(b"\0") if raw]
    # A tracked file deleted in the working tree is absent from the next commit
    # once staged; auditing its stale index blob would make cleanup impossible.
    return [
        path
        for path in paths
        if candidate_exists(root / pathlib.PurePosixPath(path))
    ]


def load_media_allowlist(path: pathlib.Path) -> dict[str, str]:
    """Load `<sha256>  <repo path>` records and reject ambiguous entries."""

    records: dict[str, str] = {}
    for line_number, raw_line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(maxsplit=1)
        if len(parts) != 2 or not re.fullmatch(r"[0-9a-f]{64}", parts[0]):
            raise ValueError(f"invalid media allowlist record at line {line_number}")
        relative = normalize_path(parts[1])
        if relative in records:
            raise ValueError(f"duplicate media allowlist path at line {line_number}: {relative}")
        records[relative] = parts[0]
    return records


def load_media_licence_rules(path: pathlib.Path) -> tuple[MediaLicenceRule, ...]:
    """Load reviewed media provenance groups from the release policy."""

    document = tomllib.loads(path.read_text(encoding="utf-8"))
    if document.get("version") != 1:
        raise ValueError("media licence policy version must be 1")
    raw_groups = document.get("asset_group")
    if not isinstance(raw_groups, list) or not raw_groups:
        raise ValueError("media licence policy must contain asset_group entries")

    rules: list[MediaLicenceRule] = []
    seen_ids: set[str] = set()
    for index, group in enumerate(raw_groups, start=1):
        if not isinstance(group, dict):
            raise ValueError(f"asset_group {index} must be a table")
        rule_id = group.get("id")
        spdx = group.get("spdx")
        source_url = group.get("source_url")
        licence_file = group.get("licence_file")
        notice_file = group.get("notice_file")
        patterns = group.get("patterns")
        if not isinstance(rule_id, str) or not rule_id:
            raise ValueError(f"asset_group {index} has no id")
        if rule_id in seen_ids:
            raise ValueError(f"duplicate asset_group id: {rule_id}")
        seen_ids.add(rule_id)
        if not isinstance(spdx, str) or not spdx:
            raise ValueError(f"asset_group {rule_id} has no SPDX licence")
        if not isinstance(source_url, str) or not source_url.startswith("https://"):
            raise ValueError(f"asset_group {rule_id} has no HTTPS source URL")
        if not isinstance(licence_file, str) or not licence_file:
            raise ValueError(f"asset_group {rule_id} has no licence_file")
        if not isinstance(notice_file, str) or not notice_file:
            raise ValueError(f"asset_group {rule_id} has no notice_file")
        if not isinstance(patterns, list) or not patterns or not all(
            isinstance(pattern, str) and pattern for pattern in patterns
        ):
            raise ValueError(f"asset_group {rule_id} has no valid patterns")
        rules.append(
            MediaLicenceRule(
                rule_id=rule_id,
                spdx=spdx,
                source_url=source_url,
                licence_file=normalize_path(licence_file),
                notice_file=normalize_path(notice_file),
                patterns=tuple(normalize_path(pattern) for pattern in patterns),
            )
        )
    return tuple(rules)


def default_private_tokens(root: pathlib.Path) -> tuple[str, ...]:
    """Current-machine path values that must never be copied into source files."""

    values = {str(root)}
    for name in ("USERPROFILE", "HOME"):
        if value := os.environ.get(name):
            values.add(value)
    values.add(str(pathlib.Path.home()))
    return tuple(sorted(value for value in values if len(value) >= 6))


def print_media_manifest(root: pathlib.Path, candidates: list[str]) -> int:
    """Print the current media hashes for deliberate allowlist review."""

    for path in sorted(candidates):
        file_path = root / pathlib.PurePosixPath(path)
        if candidate_exists(file_path) and file_path.suffix.casefold() in MEDIA_SUFFIXES:
            print(f"{hashlib.sha256(read_candidate_bytes(file_path)).hexdigest()}  {path}")
    return 0


def selftest() -> int:
    """Pin allowed code/media and every private publication refusal path."""

    failures: list[str] = []
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp).resolve()
        files = {
            ".codex/config.toml": b"project_doc_max_bytes = 262144\n",
            "crates/pixtuoid-scene/src/maple_world.rs": (
                b'const NAME: &str = "training_background";\n'
            ),
            "crates/pixtuoid-scene/sprites/default/standing.sprite": b"@frame 0\nA\n",
            "docs/images/approved.png": b"approved raster fixture",
            "docs/images/changed.png": b"changed raster fixture",
            "docs/images/unapproved.png": b"unapproved raster fixture",
            "docs/videos/unapproved.mp4": b"unapproved video fixture",
            "private-assets/skins/active-pack/market_avatar_0.sprite": b"private",
            "pack/scene_background.sprite": b"private background",
            "docs/demo.ogg": b"private audio",
            "docs/bundle.zip": b"private archive",
            "docs/setup.msi": b"private installer",
            "notes/disguised.dat": b"prefix\0private binary",
            "notes/disguised-archive.dat": b"PK\x03\x04not-text-and-no-nul",
            "notes/private-path.txt": b"C:/Users/TestOwner/private/source.png\n",
            "notes/upstream-funding.txt": b"https://buymeacoffee.com/" + b"IvanWng97\n",
            "notes/token.txt": b"github_pat_" + b"ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890\n",
            "notes/key.txt": b"-----BEGIN OPENSSH " + b"PRIVATE KEY-----\n",
        }
        for relative, content in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)

        allowlist = {
            "crates/pixtuoid-scene/sprites/default/standing.sprite": hashlib.sha256(
                files["crates/pixtuoid-scene/sprites/default/standing.sprite"]
            ).hexdigest(),
            "docs/images/approved.png": hashlib.sha256(
                files["docs/images/approved.png"]
            ).hexdigest(),
            "docs/images/changed.png": hashlib.sha256(b"original").hexdigest(),
            "docs/images/missing.png": hashlib.sha256(b"missing").hexdigest(),
        }
        findings = audit_candidates(
            root,
            list(files),
            PublicationPolicy(
                media_allowlist=allowlist,
                media_licence_rules=(
                    MediaLicenceRule(
                        rule_id="test-media",
                        spdx="MIT",
                        patterns=(
                            "crates/pixtuoid-scene/sprites/default/*.sprite",
                            "docs/images/approved.png",
                        ),
                    ),
                ),
                private_tokens=("C:/Users/TestOwner",),
            ),
        )
        observed = {(item.rule, item.path) for item in findings}
        expected = {
            ("media-hash-mismatch", "docs/images/changed.png"),
            ("media-not-approved", "docs/images/unapproved.png"),
            ("media-not-approved", "docs/videos/unapproved.mp4"),
            ("media-allowlist-stale", "docs/images/missing.png"),
            ("private-asset-path", "private-assets/skins/active-pack/market_avatar_0.sprite"),
            ("private-asset-path", "pack/scene_background.sprite"),
            ("forbidden-binary-media", "docs/demo.ogg"),
            ("forbidden-binary-media", "docs/bundle.zip"),
            ("forbidden-binary-media", "docs/setup.msi"),
            ("binary-not-approved", "notes/disguised.dat"),
            ("binary-not-approved", "notes/disguised-archive.dat"),
            ("private-local-path", "notes/private-path.txt"),
            ("upstream-funding-link", "notes/upstream-funding.txt"),
            ("credential-pattern", "notes/token.txt"),
            ("credential-pattern", "notes/key.txt"),
        }
        if observed != expected:
            failures.append(f"finding set mismatch: expected {expected}, observed {observed}")

        leaky_artifact = root / "dist" / "leaky.exe"
        clean_artifact = root / "dist" / "clean.exe"
        leaky_artifact.parent.mkdir(parents=True, exist_ok=True)
        leaky_artifact.write_bytes(b"prefix C:\\Users\\TestOwner\\.cargo suffix")
        clean_artifact.write_bytes(b"prefix /redacted/cargo suffix")
        artifact_observed = {
            (item.rule, item.path)
            for item in audit_artifacts(
                root,
                [leaky_artifact, clean_artifact],
                private_tokens=("C:\\Users\\TestOwner",),
            )
        }
        artifact_expected = {("artifact-private-path", "dist/leaky.exe")}
        if artifact_observed != artifact_expected:
            failures.append(
                "artifact finding mismatch: "
                f"expected {artifact_expected}, observed {artifact_observed}"
            )

        forbidden_allowed = {
            path
            for rule, path in observed
            if path
            in {
                "crates/pixtuoid-scene/src/maple_world.rs",
                "crates/pixtuoid-scene/sprites/default/standing.sprite",
                "docs/images/approved.png",
                ".codex/config.toml",
            }
        }
        if forbidden_allowed:
            failures.append(f"allowed paths were rejected: {sorted(forbidden_allowed)}")

        licence_findings = audit_media_licence_coverage(
            ["docs/images/approved.png", "docs/images/unlicensed.png"],
            (
                MediaLicenceRule(
                    rule_id="first",
                    spdx="MIT",
                    patterns=("docs/images/approved.png",),
                ),
                MediaLicenceRule(
                    rule_id="second",
                    spdx="CC0-1.0",
                    patterns=("docs/images/approved.png",),
                ),
            ),
        )
        licence_observed = {(item.rule, item.path) for item in licence_findings}
        licence_expected = {
            ("media-licence-ambiguous", "docs/images/approved.png"),
            ("media-licence-missing", "docs/images/unlicensed.png"),
        }
        if licence_observed != licence_expected:
            failures.append(
                "media licence coverage mismatch: "
                f"expected {licence_expected}, observed {licence_observed}"
            )

    with tempfile.TemporaryDirectory() as tmp:
        unicode_root = pathlib.Path(tmp) / "繁體中文-repo"
        unicode_root.mkdir()
        subprocess.run(["git", "init", "--quiet"], cwd=unicode_root, check=True)
        if git_root(unicode_root) != unicode_root.resolve():
            failures.append("git root must decode a UTF-8 non-ASCII worktree path")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1
    print("public-release-audit selftest passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--print-media-manifest", action="store_true")
    parser.add_argument(
        "--artifact",
        action="append",
        default=[],
        type=pathlib.Path,
        help="also reject current-machine paths embedded in this release artifact",
    )
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    root = git_root()
    candidates = git_candidates(root)
    if args.print_media_manifest:
        return print_media_manifest(root, candidates)

    allowlist_path = root / "policy" / "public-release" / "media-allowlist.sha256"
    licences_path = root / "policy" / "public-release" / "media-licences.toml"
    try:
        allowlist = load_media_allowlist(allowlist_path)
        licence_rules = load_media_licence_rules(licences_path)
    except (OSError, ValueError) as error:
        print(f"public-release-audit configuration error: {error}", file=sys.stderr)
        return 2
    private_tokens = default_private_tokens(root)
    findings = audit_candidates(
        root,
        candidates,
        PublicationPolicy(
            media_allowlist=allowlist,
            media_licence_rules=licence_rules,
            private_tokens=private_tokens,
        ),
    )
    findings.extend(audit_artifacts(root, args.artifact, private_tokens))
    if findings:
        for finding in findings:
            location = f"{finding.path}:{finding.line}" if finding.line else finding.path
            print(f"ERROR [{finding.rule}] {location}")
        print(f"public-release-audit failed: {len(findings)} finding(s)", file=sys.stderr)
        return 1
    print(
        "public-release-audit passed: "
        f"{len(candidates)} publication candidates, {len(allowlist)} approved media files, "
        f"{len(args.artifact)} artifact(s) inspected"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
