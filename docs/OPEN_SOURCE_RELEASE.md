# Open-source release checklist

This is the authoritative publication boundary for Maple Agent Market while it
transitions from a local prototype into a community project. It separates
"source available to build" from "local fan presentation enabled on one
machine" and prevents an MIT label from being applied to third-party game art.

## Release shape

The intended public repository contains:

- MIT-licensed Rust/TypeScript/JavaScript/Python/PowerShell source;
- the inherited redistributable Pixtuoid default sprite pack and documentation;
- Monaspace Neon under the SIL Open Font License 1.1;
- original programmatic arcane-claw, holy-light, dragon-pulse, command-success
  and task-completion effects, with no copied game frames;
- sprite-pack formats, converters and the local skin importer;
- tests and an automatic public-release audit.

It does not contain:

- NEXON ranking/Open API character renders or converted paperdolls;
- Free Market or Henesys screenshots, reconstructed/WZ-derived backgrounds;
- NEXON/MapleStory monsters, portals, actions, skill-effect frames or music;
- user skins, generated active packs, previews, source caches or QA captures;
- private Codex sessions, absolute workstation paths, credentials or archives.

Other players can still use the same architecture: they build the open-source
program and create/select their own local pack with `--pack-dir`. Local pack
files are not relicensed by this repository. Without one, the public-safe build
uses inherited Pixtuoid art and schematic fallbacks; it is functional but does
not reproduce the local Maple scene.

## Automatic gate

Run before every public commit candidate and release build:

```bash
python3 scripts/public-release-audit.py --selftest
python3 scripts/public-release-audit.py
python3 scripts/build-public-release.py --selftest
python3 scripts/stage-public-release.py --selftest
```

When `just` is installed, the equivalent is:

```bash
just public-release-audit
```

`just lint` and therefore `just preflight` include this gate. It examines Git
tracked files plus untracked files that are not ignored. It rejects private
asset paths, audio/archives/installers/native binaries, high-confidence
credential patterns, current-machine paths, unapproved media/binary files and
changes to an approved media file's SHA-256. Findings never print the matched
secret or private value.

A distributable host binary must be built through the path-sanitizing wrapper,
not a bare `cargo build --release`:

```bash
python3 scripts/build-public-release.py
# or: just public-release-build
```

The wrapper applies rustc path-prefix remaps, builds with the lockfile, then
runs the source audit again with `--artifact` so an executable containing a
maintainer home/workspace path is rejected. It intentionally refuses inherited
`RUSTFLAGS` because those can take precedence over the audited remap settings.

`policy/public-release/media-allowlist.sha256` is a reviewed baseline, not a
generated licence. Do not update it merely to make a red build green. For each
changed media file, first record its author, source and redistributable licence
in `THIRD_PARTY_NOTICES.md`; then review the visual content before updating the
hash. `policy/public-release/media-licences.toml` independently requires every
approved media path to match exactly one provenance/licence group and requires
the associated notice and licence files to ship.

Once the source candidate is committed, fork URLs are resolved and the tree is
clean, build a complete public-safe Windows/Linux bundle with:

```bash
python3 scripts/stage-public-release.py --output ../maple-agent-market-public
# or: just public-release-stage ../maple-agent-market-public
```

The staging command never overwrites a destination and requires the output to
be outside the repository. It runs the source/binary gates, builds with path
remapping, then emits the executable, MIT/OFL notices,
`PUBLIC_BUNDLE_MANIFEST.json` and `SHA256SUMS.txt`. The manifest states that the
bundle contains no private Maple assets. It is a packaging assertion, not a
claim that a separately supplied local pack is redistributable.

## Repository transition status

Completed safeguards:

- local Maple runtime art remains outside Git and loads through `--pack-dir`;
- a hash-bound public media inventory covers the current redistributable files;
- every approved media hash has one machine-checked licence/provenance group;
- private asset/audio/archive/path/credential checks are automatic;
- release binaries are rejected when they embed maintainer paths;
- public-safe bundle staging is exact-file, non-overwriting and dirty-tree
  refusing;
- Cargo publication is disabled for all fork crates;
- inherited tag publication, Pages deployment and paid Claude workflows fail
  closed until explicit repository variables are configured;
- upstream funding and support links are removed from fork-facing surfaces;
- Pixtuoid MIT and Monaspace OFL notices are retained.

Required before the first public source push:

- [x] Use `SaM-runtime/maple-agent-market` as the public source repository.
- [x] Create a fork-owned `origin` and retain Pixtuoid as an explicitly named
      read-only upstream remote; verify both URLs immediately before pushing.
- [x] Replace repository/homepage/crash/security links where the fork
      should receive reports; keep an explicit Pixtuoid upstream attribution.
- [x] Enable GitHub private vulnerability reporting and point `SECURITY.md` at
      the fork-owned private report form.
- [x] Keep all local Maple media outside the source tree and run the source
      audit before the first push.

Required before the first public binary/package release:

- [ ] Decide whether the public executable remains `pixtuoid` internally or
      gains a stable `maple-agent-market` binary alias without breaking config.
- [ ] Rebrand or remove the inherited website, npm, Homebrew, Raycast and
      crates.io publication surfaces. Do not enable their workflows early.
- [ ] Resolve the inherited website lockfile's current `npm audit` findings
      using its declared Node 26/npm 12 toolchain; then run the complete site
      verification before enabling Pages.
- [x] Provide a clean binary staging path that omits `private-assets`, includes
      notices plus checksums, and refuses dirty/unresolved publication input.
- [ ] Run that staging path on the final clean, fork-owned release revision and
      inspect the emitted manifest and checksum inventory.
- [ ] Use only reviewed redistributable screenshots in repository documentation;
      treat any Maple gameplay screenshot as a separate non-commercial UGC
      decision, not MIT material.
- [ ] Run fresh format, lint, dependency, test, package-content and Windows smoke
      gates from a clean publication candidate.

## Workflow locks

The inherited workflows are deliberately inert unless these repository
variables are set to the literal string `true`:

- `MAPLE_AGENT_MARKET_RELEASES_ENABLED`
- `MAPLE_AGENT_MARKET_PAGES_ENABLED`
- `MAPLE_AGENT_MARKET_CLAUDE_WORKFLOWS_ENABLED`

Do not set them merely to clear a skipped check. Each variable is the final arm
switch after its corresponding checklist items, package names, URLs, secrets,
permissions and billing have been reviewed.

## Final publication sequence

1. Freeze the code candidate; do not copy the local installation directory.
2. Run the public-release audit and full repository gates.
3. Inspect `git status`, `git diff`, `cargo metadata` and package file lists.
4. Run `scripts/stage-public-release.py --output <new-directory>` and inspect
   the exact manifest/checksum inventory; do not add the local install pack.
5. Review licence/notice files and the final README rendering.
6. Only then create the public repository or release tag.

[NEXON's Game IP guide](https://maplestoryworlds.nexon.com/en/legal/policy/1037)
describes images, backgrounds and music as NEXON Game IP and sets conditions for
attributed, non-commercial fan UGC; it does not state that those files become
MIT-licensed assets. GitHub's
[Terms of Service](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service)
place responsibility on the uploader to hold the rights needed for content they
post and license. Keeping the local pack outside the repository is therefore a
conservative engineering publication boundary, not legal advice.
