# agent-sync

Syncs AI coding agent configs from the Commons — the canonical `~/.agents/` directory — to every installed agent: symlink fan-out for identical-bytes configs (skills, instructions, commands, agents), rendered key-merge for entries in shared files (MCP, hooks), rendered whole files where formats differ.

## Language

**Commons**:
The canonical directory `~/.agents/` holding the single real copy of everything agent-sync syncs. A shared commons, not agent-sync's private directory: the path is an interop contract other agents hardcode (ADR-0004), and other tools keep their own files in it — agent-sync is a tenant, never the landlord. Distinct from agent-sync's private files (config in `$XDG_CONFIG_HOME/agent-sync/`, lock in `$XDG_STATE_HOME/agent-sync/`): the Commons is meant to be committed and synced across machines, and machine-local state must stay out of it.
_Avoid_: store, source, central repo, canonical source, agent-sync's directory

**Target**:
One agent's config surface that receives fan-out (e.g. `~/.claude/skills/`).
_Avoid_: destination, agent dir

**Fan-out**:
The mechanism: one Commons entry materialized into every target as a symlink. Always **per entry**, never a single symlink standing in for the whole target directory (ADR-0005) — a target holds one link per Commons entry, so Variants have a slot and content the agent owns is left alone.
_Avoid_: distribution, propagation

**Native (agent)**:
An agent that reads the Commons directly and therefore needs no fan-out (e.g. opencode scans `~/.agents/skills/` itself). A capability the registry claims only when it holds unconditionally: an agent that *could* read the Commons after the user edits its config is not Native, because agent-sync does not write that config.
_Avoid_: zero-config agent

**Co-tenant**:
An entry at the Commons root that is not one of agent-sync's families — another tool's file in a directory agent-sync shares (e.g. the `skills` CLI's `.skill-lock.json`). Named by `doctor`, never counted as tools (filenames carry no authorship), never reported by `status`, never touched. Not a Foreign, which is about a *target*; not an issue.
_Avoid_: foreign, stray, unmanaged, junk

**Protocol surface**:
A `.agents` Protocol path at the Commons root that agent-sync recognizes by name but does not manage (`tasks/`, `memories/`, `models.json`, `system-prompt.md`). A named kind of Co-tenant: `doctor` attributes it to the protocol instead of listing it anonymously, and it becomes a family only when real consumers exist to fan out to.
_Avoid_: extension, unknown surface

**Variant**:
A real directory (or file) in a target that intentionally shadows the Commons copy for that agent alone, while the Commons copy serves every other agent. First-class and preserved, never clobbered.
_Avoid_: override, fork, conflict, drift

**Adopt**:
Taking a path under agent-sync's management with one verb; where the path lives picks the mechanic. A real file/dir in a target is absorbed into the Commons, leaving a link behind (GNU Stow's `--adopt`). A path with a durable home elsewhere — inside a git repo — becomes a Sourced entry. Any other outside path is copied in, and the original is no longer consulted. Paths already inside the Commons refuse: they are already home.
_Avoid_: import, migrate, add

**Sourced (entry)**:
A Commons entry that is a symlink out to its **Source** — an outside path, typically in a git repo, holding the content's single real copy. The Commons keeps the pointer, the Source keeps the truth, and edits at the Source reach every agent through the Commons. The mirror image of an ordinary entry, where the Commons itself holds the real copy. (Neighboring tools use "source" for the *canonical* side — chezmoi, agentsync, dotagents; here the Source is the outside home an entry points to.)
_Avoid_: external, linked, repo-backed

**Revert**:
Removing everything agent-sync put into one target — links, merged MCP and hook keys, Markers — as a deliberate offboarding act. Refused while the target is still enabled, so a later sync cannot silently redo what revert undid.
_Avoid_: uninstall, reset, delete

**Foreign**:
A symlink, file, or MCP server entry in a target that agent-sync does not own (doesn't resolve into the Commons / name not in the Commons). Never touched, listed by `status`.
_Avoid_: unmanaged, external, orphan

**Conflict**:
A Foreign *file* occupying a destination agent-sync would otherwise write — reported with the remedy named, never overwritten. A kind of Foreign, and never a Variant (a Variant shadows a Commons entry deliberately; a Conflict blocks one).
_Avoid_: clash, collision, blocked

**Render**:
Translating a Commons MCP server into one agent's native format and key-merging it into that agent's config file. The non-symlink sync path, used only where identical bytes are impossible.
_Avoid_: compile, generate, apply

**Managed (server)**:
An MCP server entry whose name appears in the Commons — agent-sync owns that entry in every targeted config, and the Commons wins on sync.
_Avoid_: owned, tracked

**Disabled (server)**:
A Managed server whose Commons definition carries `"disabled": true` — defined but rendered nowhere. The definition and its rules are kept, so enabling restores it everywhere without retyping. Distinct from a disabled *target*, which removes an agent from management entirely.
_Avoid_: paused, off

**Tweak**:
A per-agent addition merged into one agent's rendering of a server (e.g. Codex's `startup_timeout_sec`), declared in `agent-sync.toml`, never in the Commons file.
_Avoid_: override, extra, extension

**Import-line**:
The instructions mechanism for agents whose prose file must stay user-owned *and* whose parser honors an import: ensure one additive, idempotent import line exists (Claude's `@~/.agents/AGENTS.md` in `CLAUDE.md`). The sole sanctioned edit to a user's prose file. Assigned only where the agent is measured to consume the line — an unparsed import leaves the agent with no instructions and no error.

**Include-entry**:
The instructions mechanism for agents whose config lists the instruction files to load: ensure one entry naming the Commons `AGENTS.md` exists in that list (opencode's `instructions` array; Gemini's `context.fileName` array). Owned by element identity like a hook — an entry that resolves to the Commons file is ours in whatever spelling it was written, and every other entry belongs to whoever put it there. Always appended, never placed first: for Gemini the first element is also where the agent writes its own memories, and that must never be the Commons. Chosen where a symlink at the agent's own instructions path is unsafe because another tool rewrites that file through the link.
_Avoid_: import-line, config include, instructions key

**Rules-dir link**:
The instructions mechanism for agents that glob a rules directory: drop a symlink to the Commons `AGENTS.md` into it (Roo's `~/.roo/rules/`).

**Marker**:
The one-line comment agent-sync places in every rendered whole file, making the file recognizable as agent-sync's without a state file. Marked = ours; unmarked = foreign.
_Avoid_: banner, watermark
