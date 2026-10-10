# 02 — Measure opencode when it reaches one file by both routes

**What to build:** opencode has two instructions routes (ADR-0009). The default import route is an entry in `instructions` in `~/.config/opencode/opencode.json`. The symlink route is a link at `~/.config/opencode/AGENTS.md`. `sync` keeps only the chosen one, except that it keeps the include entry while a real `AGENTS.md` blocks the link. If both ever exist at once, for example after someone re-adds the entry by hand, it is unmeasured whether opencode loads the Commons twice or only once. Measure it the way `~/.agents/docs/agent-clis.md` ("Which instruction files each agent loads") does: tools off, ask opencode to quote the first sentence of every instruction file in its context, once with both routes in place. If it loads the file twice, decide whether `doctor` should warn about it. This only matters to someone who sets `opencode = "symlink"`.

**Status:** ready-for-human

- [ ] opencode measured with both the link and the include entry in place
- [ ] The result recorded in `agent-clis.md`, and in `doctor` too if it loads the file twice
