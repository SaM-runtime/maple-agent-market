<h1 align="center">Maple Agent Market</h1>

<p align="center">
  <em>Your AI coding agents, visualized as merchants in a nostalgic 2D market.</em>
</p>

<p align="center">
  <sub><em>An unofficial side project based on Pixtuoid.</em></sub>
</p>

<p align="center">
  <a href="https://github.com/IvanWng97/pixtuoid">Pixtuoid upstream</a>
  &nbsp;·&nbsp; <a href="FORK_NOTICE.md">Attribution and asset boundary</a>
  &nbsp;·&nbsp; <a href="docs/OPEN_SOURCE_RELEASE.md">Public-release checklist</a>
  &nbsp;·&nbsp; <a href="LICENSE">MIT code license</a>
</p>

---

## Fork status and asset boundary

Maple Agent Market is an unofficial, fan-made side project built from
[Pixtuoid](https://github.com/IvanWng97/pixtuoid). Internal crate, configuration
and wire identifiers intentionally retain `pixtuoid` names as a compatibility
layer while the product-facing UI uses the new brand.

The source repository contains no local Maple media pack. MapleStory screenshots,
NEXON/Open API paperdolls, Free Market derivatives and commercial music are
local test assets and must not be committed or distributed under this
repository's MIT license.
The app can loop a user-selected MP3, WAV, OGG or FLAC natively, but it does not
download, extract or bundle YouTube audio. See [FORK_NOTICE.md](FORK_NOTICE.md).

The public-safe profile remains runnable without that local pack: it uses the
inherited MIT Pixtuoid sprites, a schematic training fallback, and original
programmatic combat/task effects. It does not reproduce the local Maple scenes
or paperdolls. `python scripts/stage-public-release.py --output <new-directory>` is
the strict binary-bundle path. It refuses a dirty tree, unresolved fork URLs,
an in-repository output directory, or an existing destination; the result
includes a machine-readable manifest, notices and SHA-256 inventory.

## Local character skin workshop (Windows prototype)

The local Windows launcher can shuffle the eight complete paperdoll slots,
lock or assign a slot, and import a user-owned nine-frame PNG skin containing
three stand, four walk and two climb poses. Imports stay under the isolated
install's `private-assets/skins` directory. The immutable base pack is copied to
a generated active pack, all nine poses are remapped together, and the existing
`validate-pack` command must pass before that pack becomes active.
The local built-in roster additionally carries NEXON-rendered `stand2`, `sit`,
and `alert` sets. A legacy nine-frame user skin reuses its own `stand1` pixels
for those optional states; it never borrows another character's body or invents
an unsupported pose.

The market state mapping is reference-gated: entering/exiting uses `walk1` and
`ladder`; the first active arrival plays one `alert` cycle, then the shop stays
open through later tool calls while `stand1` breathes behind it. Idle agents
close shop and retrace the authored platform/ladder tail before returning;
waiting agents close shop and use `sit` with the existing monitoring-only `?`
mark. A successful Codex `task_complete` / `turn_complete` event now gives only
that task's paperdoll a single 2.2-second blue level-up pillar; a tool ending,
waiting transition, or `turn_aborted` never triggers it. The pillar is
programmatic pixel art rather than bundled Maple media, and its absolute-time
animation expires instead of looping or stretching when the window is throttled.
The existing command-success flash remains a separate `exec`-specific cue.

In the Maple floating window, the shared footer data is rendered as a pink
in-game-style chat strip. Raw monitor tokens such as `A`, `W`, `I`, `Exec` and
`Bash` are translated to player-facing Traditional Chinese; the reducer and
terminal UI contracts remain unchanged.

The v21 floating view composes Free Market 1-1 and Henesys Hunting Ground I
side by side while retaining one independent scene session per map. Each root
task and its children remain on exactly one map. When the source supplies a
real `parent_id`, shop and training cards show the root task's child count or a
child's depth and root-task title; one family also shares a colored edge. An
unlinked source stays ungrouped instead of inferring lineage from its name.
`Tab` cycles through the views, while `1`/`2`/`3` select dual view, Free Market,
or training directly. `Z` (or the compact on-screen size chip) cycles small,
medium, and large window presets; manual drag and corner resize remain
available, and every preset is capped to the current monitor. Undersized dual
views automatically use the single-map fallback. Training-map entry and exit follow horizontal
platforms and the authored rope anchors, switching to the paperdoll's ladder
frames while climbing. Active combat keeps the selected paperdoll identity and
composites a rotating arcane-claw, holy-light and dragon-pulse deck across work
cycles as a separate environment layer, so a cast never replaces the paperdoll
body. The public build draws all three itself; a separately licensed local pack
may override a typed animation key. Living Slimes and Green Mushrooms use elapsed-time horizontal patrols,
turn at their platform endpoints, and keep the source move-frame timing instead
of shuffling in place.

The reusable launcher module and focused tests live under
`tools/windows/MapleSkinWorkshop.psm1` and
`tools/windows/tests/MapleSkinWorkshop.Tests.ps1`. No paperdoll or user image is
stored in this repository. This is a complete animated-skin importer, not a
Maple-Atelier-style layered hair/face/equipment compositor.

The upstream installation commands and reference documentation below remain for
compatibility during the fork transition; they are not a release of Maple Agent
Market.

> **Development status:** public source prototype for collaboration. There is
> no supported public binary, package-manager release or hosted fork demo yet.

The intended public distribution is a source repository containing the MIT
code, its inherited redistributable Pixtuoid defaults, and local import tools.
The exact Maple presentation remains an external, user-local pack. Before every
commit intended for the public repository or a release candidate, run
`python3 scripts/public-release-audit.py`;
the gate verifies Git candidates against the reviewed media hashes and rejects
private assets, audio, archives, credentials and machine-local paths. See the
[public-release checklist](docs/OPEN_SOURCE_RELEASE.md) for the remaining
repository identity and packaging decisions. A distributable executable must
be produced with `python3 scripts/build-public-release.py`, which remaps and
then audits embedded build-machine paths.

## Upstream overview (reference)

Running several coding agents means alt-tabbing between terminals to find out who's stuck, who's waiting on a permission prompt, and who finished ten minutes ago. The **Pixtuoid** foundation puts them all in one tiny pixel-art office you can watch from above — every session is a character at a desk: typing while it works, raising a `?` when it needs you, dozing off when it's done. Maple Agent Market reinterprets that state model as merchants entering, opening a shop while active, idling or sitting when work pauses, and leaving through a 2D market.

A little bit *Black Mirror*, a little bit *The Sims* — and the most glanceable multi-agent dashboard you'll ever use.

## Build the fork from source

The inherited Homebrew, npm, crates.io and `pixtuoid.dev` channels install
upstream Pixtuoid, not Maple Agent Market. Fork publication is intentionally
disabled during the transition.

With Git and Rust 1.89 or newer:

```bash
git clone https://github.com/SaM-runtime/maple-agent-market.git
cd maple-agent-market
cargo build --release -p pixtuoid
cargo run -p pixtuoid -- --help
```

To work on a change, create a branch and run the repository gate before opening
a pull request:

```bash
git switch -c feature/my-change
just preflight
```

If `just` is not installed yet, `cargo test --workspace` is the minimum Rust
test pass; the complete gate and tool bootstrap are documented in
[`CLAUDE.md`](CLAUDE.md).

Internal crate, binary and config identifiers currently remain `pixtuoid` for
compatibility. The binary runs with the redistributable upstream default pack
when no `--pack-dir` is supplied. A user-local Maple pack can be selected with
`--pack-dir <directory>`; that directory is deliberately not part of this
repository or its release artifacts.

Press `s` to open the **Sources** panel and connect an agent CLI. In another
terminal, start that coding agent. A character appears when the session is
detected; `pixtuoid doctor` reports source health while the internal command
name remains in transition.

**Keyboard shortcuts:** `q` quit · `p` pause · `s` sources (connect / health) · `t` themes · `m` sound (`+`/`-` volume) · `Tab` agent dashboard · `?` help · `↑↓/jk/PgUp/PgDn` floors · click an agent to bring its terminal to the front (`f` in the dashboard)

The inherited Pixtuoid install guide describes upstream packages, not this
fork. For now, collaborators should build this repository from source.

## Upstream capabilities retained

<!-- features:start · generated from site/src/features.json by `just gen-readme` — edit the JSON, not this table -->
| &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp; | Feature | Description |
|---|---|---|
| <img src="docs/images/pix-icons/multiagent.png" alt="" width="20" height="20"> | **Multi-agent office** | Every agent session gets its own desk — when a floor fills up, a new floor opens automatically |
| <img src="docs/images/pix-icons/multifloor.png" alt="" width="20" height="20"> | **Multi-floor office** | Hop between floors with `PageUp`/`PageDown`, `↑`/`↓`, or `j`/`k` — each switch slides into view |
| <img src="docs/images/pix-icons/spaces.png" alt="" width="20" height="20"> | **Office spaces** | Cubicles, a meeting lounge, and a pantry — the office is laid out in distinct furnished zones, not just a grid of identical desks |
| <img src="docs/images/pix-icons/walk.png" alt="" width="16" height="24"> | **Animated characters** | Coworkers type, wait with a `?`, sleep under little z's, and walk A\*-routed paths between desks |
| <img src="docs/images/pix-icons/palette.png" alt="" width="20" height="20"> | **Team palette** | Shirt and pants take their colors from the working directory — same repo, same colors, so the room reads like an org chart. Hair and skin vary per agent; 16 curated outfits |
| <img src="docs/images/pix-icons/glow.png" alt="" width="20" height="20"> | **Per-tool monitor glow** | Each desk's monitor glows with the tool in use — Edit blue, Bash orange, Read cyan — so you can read the whole room at a glance |
| <img src="docs/images/pix-icons/tokens.png" alt="" width="20" height="20"> | **Token meter** | Paper stacks up on a desk as its session burns tokens — the pile climbs through 250K / 2M / 16M tiers, a big spend drops a fresh sheet, and hovering shows the exact total (Σ) |
| <img src="docs/images/pix-icons/magnify.png" alt="" width="20" height="20"> | **Hover tooltips** | Hover an agent for session duration, tool-call count and active-time %; hover any furniture — desks, sofas, plants, vending machine, printer — for its name |
| <img src="docs/images/pix-icons/tree.png" alt="" width="20" height="20"> | **Agent tree dashboard** | Tab opens a collapsible tree of every floor's agents — each badged with the CLI it runs, color-tinted by what it's doing, with tool-call counts |
| <img src="docs/images/pix-icons/pets.png" alt="" width="20" height="20"> | **Office pets** | A cat or dog (one per floor) roams desks, pantry, sofas; sleeps near idle agents. Click to pet — pixel-art hearts float up |
| <img src="docs/images/pix-icons/lobster.png" alt="" width="28" height="24"> | **OpenClaw gateway mascot** | A live OpenClaw gateway shows up as a wandering lobster — the way it moves shows the gateway's health |
| <img src="docs/images/pix-icons/vibes.png" alt="" width="20" height="20"> | **Office vibes** | The sun and moon cross the skyline as the day goes by, weather rolls past the windows — rain, storm, snow, fog, overcast, windy, smog — and six themes give the office a whole new look |
| <img src="docs/images/pix-icons/note.png" alt="" width="20" height="20"> | **Lofi soundtrack** | A lofi soundtrack synthesized entirely in code — no audio files shipped. Day and night tracks follow the office's clock and weather, typing sounds swell with activity, and the door chime, printer and vending machine play as coworkers come and go. `m` mutes, `+`/`-` volume |
| <img src="docs/images/pix-icons/window.png" alt="" width="20" height="20"> | **Floating desktop window** | `pixtuoid floating` opens the office in a frameless, always-on-top window — on your desktop, not just in your terminal |
| <img src="docs/images/pix-icons/shield.png" alt="" width="20" height="20"> | **Hook-safe** | The tiny hook shim pixtuoid installs always exits 0 — even a stuck office can never block your agent |
<!-- features:end -->

<p align="center">
  <a href="https://pixtuoid.dev/#showcase"><strong>▶ See every feature live — floors, themes, weather, pets, the office tour →</strong></a>
</p>

## Supported Tools

<!-- tools:start · generated from site/src/sources.json by `just gen-readme` — edit the JSON, not this table -->
| Tool | Runs on |
|---|---|
| [Claude Code](https://code.claude.com) | macOS · Linux · Windows\* |
| [Codex CLI](https://github.com/openai/codex) | macOS · Linux · Windows\* |

_Also supported: [Antigravity CLI](https://github.com/google-antigravity/antigravity-cli), [DeepSeek-Reasonix](https://github.com/esengine/DeepSeek-Reasonix), [CodeWhale](https://github.com/Hmbown/CodeWhale), [Copilot CLI](https://github.com/github/copilot-cli), [opencode](https://github.com/anomalyco/opencode), [Cursor CLI](https://cursor.com/cli), [Hermes Agent](https://hermes-agent.nousresearch.com), [Oh My Pi](https://omp.sh), [OpenClaw](https://github.com/openclaw/openclaw), [Grok Build](https://github.com/xai-org/grok-build), [Kimi Code CLI](https://github.com/MoonshotAI/kimi-code)._

**→ [Full tool × OS support matrix on the site](https://pixtuoid.dev/#tools)**

_\* experimental — limited testing, unsigned binaries._
<!-- tools:end -->

> Adding a new tool? Implement the [`Source` trait](#contributing) — or, for a hook-only CLI, just a hook decoder + an install `Target` — then add a row to [`site/src/sources.json`](site/src/sources.json) (its `supported` set is pinned to the code by a test). One file, one channel, done.

## Configuration

Everything lives in `~/.config/pixtuoid/config.toml` (created on first launch;
every key optional) — theme, desk cap, custom pet names, and sprite packs. CLI
flags override the file (`pixtuoid run --theme dracula`).

The setting you'll reach for most is the **theme** — press `t` in the TUI for a
live-preview picker across six built-in palettes; your pick persists across sessions.

<p align="center">
  <img src="docs/images/themes-composite.png" alt="the six built-in themes side by side" width="800" />
</p>

See **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)** for the full key reference
(defaults, system-managed keys), the custom sprite-pack workflow, and **logging /
troubleshooting** (diagnostics go to `~/.cache/pixtuoid/log`) — or browse it live
at **[/config](https://pixtuoid.dev/config)**.

## How It Works

Agent CLIs emit events two ways — a hook shim (a 200ms fire-and-forget write to a Unix socket, or a named pipe on Windows, that can never block your agent) and JSONL transcript watching. Both feed one channel; a reducer folds events into office state; the renderer draws it as half-block pixel art. Five Rust crates, zero terminal deps in the core.

**[Full architecture with diagrams →](https://pixtuoid.dev/architecture)** · single source: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

## Privacy & Security

pixtuoid is **local-only and telemetry-free** — it makes no automatic network
connections, ships no analytics or "phone home", and reads your agent transcripts
read-only to animate the office. Your session data never leaves your machine.
The repository includes a daily `cargo-deny` advisory workflow for use after CI
is enabled. For the trust boundaries (the
hook shim, the owner-only socket, and how hook installation edits another tool's
config), see **[SECURITY.md](SECURITY.md)**.

## Contributing

PRs welcome — especially new themes, sprite/decoration polish, and `Source` adapters for agent CLIs we don't support yet (the twelve agent CLIs plus the OpenClaw gateway already wired up are in [Supported Tools](#supported-tools)). See **[CONTRIBUTING.md](docs/CONTRIBUTING.md)** for the build/test workflow, conventions, the review process, and how to add a new agent CLI. Architecture and the load-bearing invariants live in [`CLAUDE.md`](CLAUDE.md).

## Acknowledgments

Inspired by [`pixel-agents`](https://github.com/pablodelucca/pixel-agents) (VS Code), [`clawd-on-desk`](https://github.com/rullerzhou-afk/clawd-on-desk) (desktop pet), and Claude Code's [Buddy](https://dev.to/picklepixel/how-i-reverse-engineered-claude-codes-hidden-pet-system-8l7).

## License

Code is [MIT licensed](LICENSE). Local Maple/NEXON-derived packs are not covered
by that licence and are not distributed here. See
[third-party notices](THIRD_PARTY_NOTICES.md) for the retained Pixtuoid and
Monaspace attributions.
