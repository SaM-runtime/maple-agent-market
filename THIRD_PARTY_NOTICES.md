# Third-party notices

Maple Agent Market is a source-code fork. This file covers material in the
intended source publication candidate; local Maple packs are deliberately
excluded and are documented in `FORK_NOTICE.md`.

## Pixtuoid

- Project: [Pixtuoid](https://github.com/IvanWng97/pixtuoid)
- Copyright: Copyright (c) 2026 Ivan Wang
- Licence: MIT, retained in `LICENSE`
- Use: foundation code, documentation, default pixel sprites, generated site
  media and compatibility identifiers

The upstream copyright and permission notice must remain in redistributed
copies or substantial portions. Maple Agent Market modifications add a second
copyright line without replacing the upstream notice.

## Monaspace Neon

- Project: [Monaspace](https://github.com/githubnext/monaspace)
- Copyright: Copyright (c) 2023 GitHub
- Licence: SIL Open Font License 1.1
- Files: `crates/pixtuoid/fonts/MonaspaceNeon-SemiBold.otf` and
  `crates/pixtuoid/fonts/OFL-Monaspace.txt`

## Reviewed media inventory

`policy/public-release/media-allowlist.sha256` pins the exact bytes reviewed for
publication. `policy/public-release/media-licences.toml` maps every approved
media path to exactly one provenance/licence group and requires its licence and
notice files to remain in the repository. A path match is not a licence by
itself; changing any media hash still requires source and visual review.

The arcane-claw fallback, holy-light burst, dragon-pulse, command-success flash
and task-completion pillar are original programmatic pixel effects in the Rust
source. They do not introduce a third-party asset or separate asset licence.

## Material not distributed

NEXON/Open API paperdolls, MapleStory.io/WZ-derived map or effect images,
reconstructed game scenes, player-supplied skins, music, previews and QA
captures are not part of this repository's MIT or OFL material. Their absence
is enforced by `scripts/public-release-audit.py` and the reviewed media hashes
under `policy/public-release/`.
