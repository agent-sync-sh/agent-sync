## Agent skills

### Issue tracker

Issues live as local markdown files under `.scratch/<feature-slug>/` in this repo. See `docs/agents/issue-tracker.md`.

### Triage labels

Default canonical labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

The `CONTEXT.md` vocabulary is binding for code identifiers, docs, and CLI output — the central directory is the **Commons** (never "store" or "source"), and each term's _Avoid_ list is enforced by the output-vocabulary test. Consequential calls the spec didn't settle go to `DECISIONS.md` (append-only, dated prose entries). The current version's spec is the highest-numbered `.scratch/agent-sync-v*/spec.md`.
