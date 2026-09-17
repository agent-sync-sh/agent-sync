# agentstow

> [!IMPORTANT]
> **agentstow has been renamed to `agent-sync`.**
> This is the final release under the old name. The project continues as
> **`agent-sync-sh`** on crates.io, npm, PyPI and Homebrew — the command you run
> is `agent-sync`. New home: <https://agent-sync.sh>
>
> Nothing in your `~/.agents/` Commons changes; it is name-neutral by design.

**One canonical .agents/ folder, fanned out to all your AI coding agents.**

Website and docs: <https://agentstow.dev/>

The Commons — the canonical `~/.agents/` directory — holds the single real copy of every
config you share: skills, instructions, MCP servers, slash commands, agents and hooks.
`agentstow sync` fans it out to every agent you actually have installed.

Configs that can be byte-identical everywhere are **symlinked**, so there is one file seen
from ten places and drift is impossible by construction. Configs that cannot be — MCP
servers and hooks, which live inside files the agent also owns, in formats no two agents
share — are **rendered and key-merged**, with your other keys preserved.

There is no state file. There never will be. The filesystem is the state.

```
~/.agents/
├── skills/<name>/       fanned out as symlinks
├── commands/<name>.md
├── agents/<name>.md
├── AGENTS.md            symlink, import-line, or rules-dir link per agent
├── mcp.json             rendered into each agent's native dialect
└── hooks/<Event>.toml   merged by command string
```

## Install

```sh
npm install -g agentstow      # prebuilt binary, macOS, Linux and Windows, no toolchain
pip install agentstow         # the same binary, shipped as a wheel
cargo install agentstow       # from source, needs Rust 1.97+

npx agentstow doctor          # or try it first, without installing anything
uvx agentstow doctor          # the same, if you reach for uv rather than npm
```

On macOS and Linux there is also a Homebrew tap. It lives in this repository rather
than a separate `homebrew-agentstow` one, so it is tapped by URL:

```sh
brew tap agentstow/tap https://github.com/agentstow/agentstow
brew trust agentstow/tap      # Homebrew 6 refuses to load untrusted third-party taps
brew install agentstow
```

## Use

```sh
agentstow init            # create the Commons and report what this machine already has
agentstow adopt <path>    # take a path under management; --dry-run names the mechanic
agentstow sync            # fan out; --dry-run prints the full plan
agentstow status          # what is linked, what is not, and what is not ours
agentstow doctor          # installed agents, Commons hygiene, Sourced entries
agentstow revert <agent>  # offboard one agent (refuses until you disable it)
agentstow mcp list | adopt | remove | enable | disable
```

`adopt` picks its mechanic from where the path lives: a real config inside an agent's
directory is **moved** into the Commons with a link left behind; a path inside a git repo
becomes a **Sourced** entry — the Commons links out to it and the repo keeps the truth;
anything else is **copied** in. To gate CI on drift use `status` (exit 0 clean, 2
actionable, 1 error) — `sync --dry-run` previews, it does not gate.

## Agents

Claude Code, Codex, opencode, pi, oh-my-pi, OpenClaw, Hermes, Gemini CLI, Cursor, Windsurf,
Roo and Cline. Detection is simply whether the agent's config directory exists — agentstow
never creates one. Agents that read `~/.agents/skills` natively (Codex, opencode, oh-my-pi,
Gemini CLI, Cursor) get no skill links written, because nothing needs to be — and where an
agent still reads its old fan-out directory beside the Commons (Codex, Cursor), `sync`
prunes agentstow's now-duplicate links from it.

## Interop

The Commons speaks the conventions that won: `skills/<name>/SKILL.md` — the layout
opencode and friends read natively — one `AGENTS.md`, and an `mcp.json` in the standard
`mcpServers` shape. `.agents` Protocol surfaces (`tasks/`, `memories/`, `models.json`,
`system-prompt.md`) are recognized by `doctor` and never touched.

## What it will not touch

Anything it does not own. A symlink pointing outside the Commons, a hand-written file, an
MCP server whose name isn't in the Commons — all Foreign, all reported by `status`, none
ever modified. A real directory shadowing a Commons entry is a **Variant**: deliberate,
preserved, and counted as actionable only when its contents are identical to the Commons
copy, so you can dedupe on purpose.

The Commons is exactly that — a commons, not agentstow's alone. opencode, oh-my-pi and
Hermes read `~/.agents/` themselves, and other tools keep their own files there. `doctor`
names entries that aren't agentstow's and leaves them be; `status` stays target-only,
since a neighbour's file has no fan-out to report.

## Configuration

Optional `agentstow.toml` in `$XDG_CONFIG_HOME/agentstow/` (default `~/.config/agentstow/`):
disable targets, define custom ones, scope MCP servers per agent, per-agent Tweaks. The
lock — agentstow's only machine state — lives in `$XDG_STATE_HOME/agentstow/` (default
`~/.local/state/agentstow/`). Environment: `AGENTSTOW_HOME` relocates the Commons (doctor
warns — native readers won't follow), `AGENTSTOW_TARGET_ROOT` resolves everything against
another root, `AGENTSTOW_LOCK_TIMEOUT_MS` bounds the lock wait.

## What it does not do

No cross-machine sync — version the Commons with git or chezmoi. No undo — refusals come
before writes, `sync` plans everything before writing anything, `sync` and `adopt` preview
with `--dry-run`, re-runs are idempotent, and a git-versioned Commons is better history
than any journal. **No memory sync**: agent memory is not a defined artifact and agentstow will
not pretend otherwise. No GUI, daemon or file watcher. It does not install skills; it fans
out whatever is in the Commons, whoever put it there.

On Windows, creating symlinks requires Developer Mode (Settings → System → For
developers) or an elevated shell; `sync` says exactly that when it cannot link.

## Name and inspiration

The name is a nod to [GNU Stow](https://www.gnu.org/software/stow/), the classic symlink-farm
manager — agentstow does for agent configs what Stow does for dotfiles. The project is also
inspired by two neighbours in the same space:
[dotagents](https://github.com/iannuttall/dotagents) and
[agentsync](https://github.com/spxrogers/agentsync).

[Documentation](https://agentstow.dev/docs) · [agentstow.dev](https://agentstow.dev) · MIT
