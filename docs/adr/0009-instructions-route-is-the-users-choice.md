# Each agent's instructions route is the user's choice, and Claude defaults to the import line

ADR-0008 gave each agent exactly one instructions mechanic, chosen from measured parser behavior. The owner then asked that every agent offer a choice between a **symlink** route and an **import** route, set per agent in `agent-sync.toml`, with symlink as the default where the agent safely supports it and import otherwise. So each registry row now lists the routes its agent can take, default first:

```toml
# $XDG_CONFIG_HOME/agent-sync/agent-sync.toml
[instructions]
claude = "symlink"   # or "import"; omitted, the agent's default
```

`symlink` names a Symlink or Rules-dir link (the Commons file itself at the agent's path). `import` names an Import-line or Include-entry (a reference from a file the agent already reads). Each Target resolves its route once, and `sync`, `status`, `doctor`, `adopt` and `revert` all read that answer.

| agent | routes, default first | why the default |
|---|---|---|
| claude | import (Import-line in `CLAUDE.md`), symlink (`CLAUDE.md`) | Cowork skips a symlinked `CLAUDE.md`, below |
| opencode | import (Include-entry in `opencode.json`), symlink (`AGENTS.md`) | ADR-0008: claude-mem writes through the link |
| gemini | import (Include-entry in `settings.json`), symlink (`GEMINI.md`) | ADR-0008, and Gemini's memory tool writes `GEMINI.md` |
| codex, pi, oh-my-pi, windsurf | symlink | no measured import route; Codex is measured not to expand `@` |
| roo | symlink (Rules-dir link) | no import route |

**Why Claude defaults to the import line.** Claude Code's memory docs (<https://code.claude.com/docs/en/memory>, the external-imports warning) say that in Claude Cowork desktop sessions it skips a `~/.claude/CLAUDE.md` that is itself a symlink or hard link. Under the symlink route Cowork loads no user-scope instructions at all. The same paragraph says Cowork also skips any import in a user-scope file that resolves outside the session's working directory, so under the import route Cowork loads `CLAUDE.md`'s own text but not the Commons. The owner accepted that limit; no copy route is added.

**Why an agent cannot take a route it lacks.** `codex = "import"` is a config error, not a written line. An import the agent does not expand leaves it with no instructions and no error (ADR-0008), so only measured routes are offered.

**opencode and Gemini on the symlink route.** The owner chose to allow it, as an opt-in with the hazard documented. claude-mem writes both files through a link, and Gemini's own memory tool writes element 0 of `context.fileName`, which is `GEMINI.md` when agent-sync does not touch that list. On this route both writes land in the Commons. The include-entry stays their default.

## Switching routes

`sync` converges a switch, so the agent reads the Commons by one route:

- What the route not taken left of ours is removed: our symlink, or our include entry. This happens only while the chosen route is not blocked by a Conflict or a Foreign link, so a blocked switch never leaves the agent with neither. If a write for the chosen route fails, `sync` skips that agent's remaining items for the run.
- Claude's two routes share one path, so the swap there is one step (state `replace`). A `CLAUDE.md` that is our link to the Commons becomes a file holding only the import line. A `CLAUDE.md` holding nothing but that line becomes our link. Any other real `CLAUDE.md` is a Conflict whose note names both fixes, and it is never rewritten.

That second rule adds an ownership identity beside ADR-0003's three: **a file holding nothing but our import line is ours**. agent-sync wrote it, and it carries none of the user's text.

## The bug this closes

The owner's fleet links `~/.claude/CLAUDE.md -> ../.agents/AGENTS.md` by hand. agent-sync 1.1.0 read through that link, found no `@` line, and `sync` would append `@~/.agents/AGENTS.md` into the Commons `AGENTS.md` itself. `revert` had the mirror bug: removing the line followed the same link and rewrote the Commons. Now the import route treats a link to the Commons as the other route's artifact and replaces it, a link to any other Commons file is a Conflict, and removing the import line never follows a link into the Commons. On the first `sync` after release, each fleet host's linked `CLAUDE.md` becomes a file holding the import line, unless that host's config sets `claude = "symlink"`.

## Considered and rejected

- **Claude on the symlink route by default** (the unmerged `claude-md-mechanic` branch, whose own ADR-0009 this one replaces). It matches the fleet's hand-made links, but Cowork then loads no user-scope instructions, and a user's own `CLAUDE.md` turns into a Conflict on upgrade.
- **A copy route** that renders the Commons text into `CLAUDE.md` behind a Marker, so Cowork reads it. It is the only route Cowork honors in full, but it is the rendered-copy drift ADR-0001 refused. The owner declined it.
- **Refuse the symlink route for Gemini**, because the Include-entry glossary says its first element must never be the Commons. The owner chose the opt-in. The glossary line now applies to the include-entry route only.
- **Detect the route from the filesystem** (a link means symlink, a file means import). It is stateless, but a tool that replaces the link with a file would then silently flip the route. The config key is explicit and survives that.

## Consequences

- `status --json` names each instructions item's `path`, since one agent can now have two items (the chosen route and a leftover).
- `revert` undoes every route in the agent's row, not only the chosen one. Each removal touches only what is ours.
- `doctor` checks for an inert import line wherever the agent has no import-line route at all. Claude's line is never called inert, on either route.
- Windows without Developer Mode: the `replace` swap removes the import-only file before creating the link. If the link fails, only our line is lost and the error is reported.
- Re-measure if Claude Code changes how Cowork loads `~/.claude/CLAUDE.md`, or if an agent listed as symlink-only gains a measured import route.
