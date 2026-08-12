# Maple Agent Market fork notice

Maple Agent Market is an unofficial, fan-made agent-orchestration visualizer
based on [Pixtuoid](https://github.com/IvanWng97/pixtuoid).

It is not affiliated with, endorsed, sponsored or approved by NEXON, Gamania or
the Pixtuoid maintainers. MapleStory and related names and marks belong to their
respective owners.

## Code license and attribution

The Pixtuoid-derived source remains under the MIT license in `LICENSE`. The
original `Copyright (c) 2026 Ivan Wang` notice must remain with redistributed
copies. A future project copyright notice may be added for original changes; it
must not replace the upstream notice.

## Assets not covered by the MIT license

Do not publish or bundle the local Maple-inspired test pack, including:

- NEXON ranking or Open API paperdolls and their generated `.sprite` files;
- Free Market/Henesys screenshots, reconstructed or WZ-derived backdrops;
- NEXON-derived shop frames, monsters, portals, character actions and
  skill-effect art;
- MapleStory music, YouTube downloads, extracted client/WZ assets or QA images
  containing those materials.

Those files belong in the install-local `private-assets` or local sprite-pack
folders only. They are intentionally absent from the source repository.

### Local cache lifecycle

Every NEXON renderer response and every derivative or copy made from it inherits
the deadline in its local `source-manifest.json`. This includes PNG sources,
`.sprite` frames, previews, caches, backups, and QA screenshots. The applicable
local manifest owns each cache's retrieval and refresh/delete deadline. A
refresh means fetching the source again, rebuilding all derivatives, recording
new hashes and deadline, and removing old copies. Temporary dates and local
provenance records do not belong in the public source repository. These files
must never be committed or added to release artifacts.

The character skin workshop follows the same boundary. User-supplied PNGs,
converted `.sprite` files, previews, settings and generated active packs remain
under `private-assets/skins`; only the importer code and format documentation
may be published with the MIT source.

## Intended public release model

The public repository may contain the MIT code, its inherited Pixtuoid default
sprites, the OFL-licensed Monaspace font, format documentation and local import
tools. Original programmatic combat/task effects in the source are also MIT
code and contain no copied game frame. Other players can build or select a
private pack on their own machine; the repository and official releases do not
host that pack.

This separation does not claim that non-commercial fan UGC is prohibited. It
keeps NEXON-derived files out of the project's open-source licence and prevents
downstream users from mistaking them for assets they may freely sublicense or
sell. Screenshots or demonstrations require separate, deliberate UGC review and
clear unofficial/non-commercial attribution.

## Local BGM contract

The app supports native looping playback of a user-selected MP3, WAV, OGG or
FLAC. The user is responsible for choosing a file they have the right to use.
The launcher copies it into the isolated install's `private-assets/bgm` folder;
that folder is excluded from Git. The project contains no YouTube downloader,
audio extractor or hidden web player.
