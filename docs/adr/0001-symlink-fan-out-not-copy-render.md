# Symlink fan-out with zero state, not copy+render

agentsync (the closest prior art) copies + renders + per-key-merges configs into each agent's native files, tracking ownership in a state file — which buys format translation, per-key merging into shared files, and secret injection, at the cost of drift classes, a reconcile workflow, and a documented foot-gun where hand-edits to owned keys are silently destroyed. agentstow takes the opposite bet: every synced config exists exactly once (in `~/.agents/`), targets hold only symlinks, and there is **no state file, permanently** — the filesystem is the state, `sync` is "ensure a link per store entry, prune dangling store links," and drift is impossible by construction because there is only one file.

## Consequences

- We knowingly give up format translation, per-key merges, and secret injection. Configs that are byte-identical across agents (skills, AGENTS.md) fit perfectly; anything needing per-agent rendering does not.
- MCP sync (v2) cannot be symlinked (per-agent formats, embedded in files the agent also owns, e.g. Codex's `config.toml`). The statelessness principle constrains its design to stateless approaches (whole-owned-file generation, marker-delimited blocks) — never a per-key merge engine with ownership tracking.
- The sole sanctioned edit to a user-owned file is the additive, idempotent `@~/.agents/AGENTS.md` import line in `~/.claude/CLAUDE.md` (Claude can't symlink its instructions because users keep Claude-specific content there).
- Windows is out of scope for v1 (symlink creation requires Developer Mode/elevation); the link-materialization step is kept swappable in case a copy mode is ever demanded.

---

*Amended 2026-08-16: "the Store" is renamed **the Commons** (DECISIONS.md 2026-08-16); wording above is preserved as written. Windows shipped in 1.2.0.*

*Amended 2026-09-18: the "sole sanctioned edit" is now the sole sanctioned edit to a user's **prose** file. opencode and gemini take an **Include-entry** in their own config's list of instruction files instead of a symlink, because another tool rewrites their instructions file through a link (ADR-0008); wording above is preserved as written.*
