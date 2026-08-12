# Security Policy

## Supported Versions

Maple Agent Market is currently a public source prototype. There is no
supported binary or package channel yet. Until the first release, build and
test the current source revision.

| Version        | Supported |
|----------------|-----------|
| latest         | ✅        |
| older releases | ❌        |

## Privacy posture

Maple Agent Market retains Pixtuoid's **local-only and telemetry-free** design:

- **No automatic network egress.** The process makes no outbound network
  connections for analytics, crash-reporting upload, update checks, or "phone
  home". Clicking an explicitly displayed project link can ask the operating
  system to open the user's browser. The crash hook writes a backtrace to a
  *local* file. Optional BGM and sprite packs are also local files. The
  repository includes a `cargo-deny` workflow (see `.github/workflows/audit.yml`).
- **Your session data stays on your machine.** pixtuoid reads your agent CLIs'
  transcripts (e.g. `~/.claude/projects`, `~/.codex/sessions`) **read-only**, to
  derive what each sprite is doing; nothing is transmitted anywhere. The local
  Maple asset workshop is outside the public source/release boundary described
  in `FORK_NOTICE.md`.

## Trust boundaries / attack surface

The components that handle untrusted or privileged input, and how they're bounded:

1. **The hook shim (`pixtuoid-hook`)** is invoked by your agent CLI and forwards a
   single JSON line from stdin to the office over a **Unix domain socket** (a
   per-user runtime path — `$XDG_RUNTIME_DIR/pixtuoid.sock`, else a per-user
   `0700` directory `/tmp/pixtuoid-<uid>/pixtuoid.sock`; `PIXTUOID_SOCKET`
   overrides — the socket itself created `0600`, owner-only). The `/tmp` fallback
   dir is created with a TOCTOU-safe `mkdir` + ownership/mode validation, and the
   shim verifies the connected peer's uid (`getpeereid`/`SO_PEERCRED`) before
   writing — so another user cannot squat the (formerly flat, predictable)
   rendezvous path to disable the hook plane, nor intercept the payload by racing
   a listener onto it. On **Windows** the transport is a named pipe
   `\\.\pipe\pixtuoid-<user>` with an owner-only DACL, and the shim likewise
   verifies the pipe **server's** token user SID matches ours
   (`GetNamedPipeServerProcessId` → token compare) before writing — closing the
   same squat-and-intercept vector on the machine-global pipe namespace (#495).
   It is a *local IPC* — there is no network
   listener. The shim is hardened to **never block the agent**: it always exits
   `0`, within a hard ~200 ms watchdog bound, on any error. It does not execute or
   shell out to anything in the payload.

2. **The office's socket listener** binds that owner-only socket with a flock'd,
   `O_NOFOLLOW`, atomic-rename bind — only the local user can connect. The CLI's
   plain-text surfaces (the `--headless` summary, the `doctor` report, the
   Sources/install output) **strip control characters** from untrusted wire values
   (`strip_control_chars`) so a crafted transcript can't smuggle escape sequences
   into piped output. The live half-block TUI renders into its own cell grid (agent
   labels derive from project-directory path components, read over the user's own
   `0600` socket and transcripts), not by echoing raw bytes to the terminal.

3. **Hook installation** (when you explicitly *connect* a source) edits the agent
   CLI's own config — e.g. `~/.claude/settings.json` — through a single
   advisory-locked, `fsync` + atomic-rename writer that **preserves the file's
   permissions, follows stow symlinks, and takes a one-time backup** before the
   first change. Installs are idempotent and reversible (*disconnect* removes the
   hook entries via a sentinel). pixtuoid never writes another tool's config
   except on an explicit connect/disconnect.

If you find a way to cross one of these boundaries (e.g. a transcript or hook
payload that escapes the terminal, a non-owner socket connect, an install path
traversal, or any network egress), please report it.

## Reporting a Vulnerability

Use this repository's [private vulnerability reporting
form](https://github.com/SaM-runtime/maple-agent-market/security/advisories/new).
Do not send fork-only reports to the upstream Pixtuoid tracker. If the issue
also reproduces in unmodified upstream Pixtuoid, coordinate a separate upstream
report without disclosing an unpatched vulnerability publicly.

No response-time SLA is promised during the prototype phase.

**Do not** open a public issue for security vulnerabilities.
