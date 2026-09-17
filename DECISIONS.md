# Decisions

Append-only. Calls the spec did not settle, made while implementing v1.

## 2026-08-13 — Conflict is a kind of Foreign, not a Variant

`CONTEXT.md` lists "conflict" under _Avoid_ for **Variant**, but the spec and
ticket 04 both call a Foreign-owned instructions file a conflict. Resolved by
defining **Conflict** in the glossary as its own term — a Foreign *file*
blocking a destination — rather than dropping either word. A Variant shadows a
Store entry on purpose; a Conflict blocks one. Both stay distinct from each
other and from plain Foreign links.

## 2026-08-13 — Conflicts are reported but not "actionable"

`status` exits 2 only for states `sync` can resolve. A Conflict needs a human
decision about another tool's content, so counting it would leave any machine
where claude-mem owns `~/.config/opencode/AGENTS.md` permanently red, training
users to ignore the exit code. Same reasoning as a diverged Variant.

## 2026-08-13 — Instructions mechanisms are per-agent; conflict is a runtime state

The spec's capability matrix lists opencode and Gemini instructions as
"conflict". That describes the state observed on the author's machine, not a
mechanism. The registry therefore gives both `Instructions::Symlink`, and a
Foreign file occupying the path is detected at runtime. Any agent can be in
conflict; none is inherently conflicted.

## 2026-08-13 — `init` implemented ahead of its ticket, scaffold only

Tickets 01–07 all print "run `agentstow init`" when the Store is missing, which
was false advice while ticket 13 was unimplemented. The scaffolding half of
`init` was pulled forward so the message is true. The guided first-run report
(detected agents, adoption candidates, conflicts) remains ticket 13, which is
still blocked by MCP adoption.

## 2026-08-13 — A third environment override, `AGENTSTOW_LOCK_TIMEOUT_MS`

The spec's seam names two overrides. Ticket 02 requires proving that a second
process fails cleanly on a held lock, which is untestable against a 30 second
default. The override is test-facing; the two documented overrides remain the
only ones that redirect paths.

## 2026-08-13 — Writability is asked of the OS, not inferred from mode bits

`doctor` uses `access(2)` with `W_OK` rather than `permissions().readonly()`,
which reports false for a directory owned by another user at 0755 — precisely
the case worth catching. Unix-only, which matches the v1 platform scope.

## 2026-08-13 — Registry rows still needing live verification

The spec flags rows sourced from agentsync's adapters rather than observation.
Verified during implementation, against the installed agents on the author's
machine or the agent's own source: opencode (`agents/`, `commands/`, native
skills, unknown-key preservation), oh-my-pi (native store scan), pi (no MCP by
design), Codex/Cursor/Roo/Windsurf fan-out directories.

Still unverified, and deliberately unused until their tickets: Codex hooks —
the registry says `.codex/hooks.json` (observed on the machine) while agentsync
targets inline `[hooks.*]` in `config.toml`. Both may be true, since trust
metadata lives in `config.toml` regardless. Pin this before ticket 11.

## 2026-08-13 — `sync` never deletes an MCP server

Without state, a name that has vanished from the Store is indistinguishable
from one the user added by hand — both are simply "in the config, not in the
Store". Deleting on that evidence would eventually delete someone's hand-added
server. So `sync` adds and restores but never removes; removal is the explicit
`mcp remove` (ticket 10). The cost is that a server dropped from the Store
lingers until asked for by name, and `status` calls it Foreign.

## 2026-08-13 — Ticket 08 renders only the standard JSON dialect

The registry declares Codex as TOML and four agents as non-standard dialects,
but only the standard JSON shape has a renderer until ticket 09. Rendering was
therefore gated on `Format::Json && McpDialect::Standard`. Without the gate,
`sync` wrote a JSON document into `~/.codex/config.toml` — a file that also
holds the user's model and sandbox settings. Found by adversarial review, not
by the tests, which only ever installed one MCP-capable agent.

## 2026-08-13 — An unparseable agent config is skipped, not fatal

A destination agentstow cannot parse is exactly the file it promised never to
touch. Aborting the whole survey meant one ordinary `~/.codex/config.toml`
denied service to every other agent and made `status` print nothing at all —
including under `--json`. Now the Target is skipped, the fault is reported, and
every healthy Target still syncs. The exit code is still non-zero, because the
user asked for work that did not happen.

## 2026-08-13 — Cross-file atomicity is not attempted

Each config file is replaced atomically, but a run touching several agents is
not a transaction: if the second write fails, the first stands. Everything is
resolved before any file is opened, so the remaining writes are still correct,
and each failure is reported individually. Rolling back a successful write
would mean restoring a file another program may already have changed.

## 2026-08-13 — The Store's `mcp.json` has exactly one shape

Accepting a bare map as well as the standard `{"mcpServers": {...}}` wrapper
read leniently but failed dangerously: a `$schema` key becomes a server named
`$schema`, and a typo in the wrapper silently syncs nothing. One documented
shape, and a clear error otherwise.

## 2026-08-13 — File modes are preserved, exposure is reported

A rendered file inherits the mode it already had; silently tightening a file
another program owns is its own surprise. But resolved secrets landing in a
group- or world-readable file is worth saying out loud, so that combination
warns. New files are created 0600.

## 2026-08-13 — No re-stat before rename

Claude Code rewrites `~/.claude.json` continuously, so a sync during a live
session can lose whichever side finishes second. Detecting this (re-stat
immediately before `rename`, bail on change) was considered and rejected for
v1: the spec chose a documented "don't sync mid-session" caveat, and a spurious
failure is worse than a rare race for a command users run deliberately.

## 2026-08-13 — Which dialect translations are lossy, and why that is accepted

Rendering is one-way for now, but the losses matter for the eventual `mcp adopt`
of a rendered entry:

- **Codex** has one remote transport, so `http` and `sse` render identically.
  Reading one back cannot tell which it was.
- **OpenCode** has `local`/`remote` only, the same collapse, and flattens
  `command` + `args` into a single array, so the boundary between the executable
  and its first argument is gone.
- **Windsurf** emits `serverUrl` with no type key — again, transport identity is
  not recoverable.
- **Gemini** is the exception: `url` means SSE and `httpUrl` means streamable
  HTTP, so its transport round-trips.

Nothing is dropped that the canonical entry declared; only the *distinction*
between two transports is, and only where the agent itself has no way to express
it. Keys agentstow does not model pass through verbatim to every dialect.

## 2026-08-13 — TOML parse failures are named by position, never quoted

`toml_edit::TomlError`'s `Display` includes the offending source line. These
files hold resolved secrets, so formatting one leaked a credential to stderr in
`status`, `status --json`, `sync` and `sync --dry-run` alike. Errors now carry
line and column derived from the error's span and nothing else. Found by
adversarial review; the same discipline the JSON path already followed.

## 2026-08-13 — Codex renders drop null-valued keys

TOML has no null, so the merge cannot write one. Leaving nulls in the rendered
entry meant the written file never matched what was rendered, so Codex reported
Drifted on every run and `status` never returned 0. Nulls are now stripped from
the Codex render itself, which keeps the comparison symmetric with what the file
can hold. The five JSON dialects still pass nulls through harmlessly.

## 2026-08-13 — An integer TOML cannot hold is refused, not approximated

TOML integers are signed 64-bit. A Store value above `i64::MAX` was being
written as a float, which both corrupted it and left the entry permanently
drifted. It is now an error naming the key, and nothing is written.

## 2026-08-13 — The user's TOML formatting is theirs

Three things the merge preserves that it did not at first: a comment written
above a Managed server (it lives in the table's decor, not the key's), a bare
`[mcp_servers]` header the user typed by hand, and the file's own line endings
and byte-order mark. None of these change what the file means, which is exactly
why silently rewriting them would be an unrequested edit.

## 2026-08-13 — The tool config schema for MCP: `agents` and `tweaks`, nothing else

`[mcp.<name>] agents = [...]` scopes a server; `[mcp.<name>.tweaks.<agent>]`
holds native knobs for one agent. The reserved `tweaks` segment avoids the
ambiguity in a flatter `[mcp.<name>.<agent>]`, where adding an agent to the
registry would silently change what an unedited config file means. Inside
`[mcp.<name>]` only those two keys are accepted; anything else is an error, so
a typo is loud rather than ignored forever.

## 2026-08-13 — Adoption records what is, rather than deciding to spread it

`mcp adopt` writes an allowlist naming only the agents it actually absorbed
from, so the following `sync` is a genuine no-op. Adopting a server that lives
in one agent does not silently push it to the other five; the message says how
to widen it. The allowlist is built from what was absorbed, never from what was
asked for — naming an agent whose entry was refused would let the next sync
overwrite that agent's server with another agent's version.

## 2026-08-13 — An adoption that would not survive a sync is refused

`mcp adopt` renders its own result back through the dialect and compares it
against the entry it read. Anything that differs means the next sync would
change that file, so the adoption is refused rather than reported as success.
The comparison deliberately skips `${env:VAR}` resolution: a literal reference
in an agent's own config must compare equal to itself, or every such server
would read as unfaithful.

## 2026-08-13 — Ambiguous transports are recorded by omission

Codex, OpenCode and Windsurf cannot express the difference between http and
sse, so adopting a remote server from them records no `type` at all. Omission
re-renders identically, which is what makes the no-op guarantee hold; a guess
would not. Gemini encodes the transport in its URL key, so adoption from it is
exact. Every omission is reported.

## 2026-08-13 — Adoption warns when it copies something that looks like a secret

Values are taken verbatim out of an agent's config, so a credential typed there
by hand lands in the Store — the file this design encourages committing. When a
key under `env` or `headers` looks like a credential and holds a literal value,
adoption says so by key name, never by value, and suggests `${env:VAR}`.

## 2026-08-13 — `mcp remove` is the only deletion, and it is thorough

It clears the server from every Target, from the tool config (allowlist and
Tweaks alike) and from the Store, in that order, so no intermediate state is
incoherent. It refuses a name the Store does not have: a server agentstow never
managed is Foreign, and deleting Foreign entries is exactly what the rest of the
tool promises never to do. JSON removal uses `shift_remove`, because with
`preserve_order` a plain `remove` swaps the last entry into the hole and
scrambles the user's file.

## 2026-08-13 — Codex hook rows verified against the live agent

The earlier entry flagged these as unverified. Confirmed on this machine:
hook *definitions* live in `~/.codex/hooks.json` (JSON, top-level `hooks` key,
the same nested shape Claude uses), and `[features] hooks = true` enables them.
The registry rows were already correct. Claude's own hooks live in
`~/.claude/settings.json` and Gemini's in `~/.gemini/settings.json`, both under
a `hooks` key with the same shape.

## 2026-08-13 — Trust safety is structural, not behavioural

Codex records a `trusted_hash` per hook in `~/.codex/config.toml` — a
*different* file from the `hooks.json` that holds the definitions. agentstow
writes only the latter, so "never write trust metadata" holds by construction
rather than by care. A test asserts `config.toml` is byte-identical after a
sync, and the same was confirmed against the real file.

The hash formula itself was not reverse-engineered: eight candidate
serialisations (the hook object, the group, the whole file, the command, the
referenced script) all failed to reproduce it. That does not weaken the
guarantee, but it does mean agentstow cannot *predict* whether Codex will
re-prompt — only that it never grants trust itself.

## 2026-08-13 — Claude and Codex share a hook vocabulary; Gemini does not

Codex's snake_case trust keys (`pre_tool_use`, `user_prompt_submit`) are
lowercased forms of the PascalCase names in `hooks.json`, which match Claude's.
So the canonical event name is Claude's, matching the choice already made for
commands and subagents. Gemini uses its own words for some of the same moments
(`BeforeTool`, `AfterTool`, `AfterAgent`, `PreCompress`) and has no equivalent
for others; an event an agent does not have is skipped and reported as
`unsupported`, never invented.

## 2026-08-13 — The hooks family is "in use" when its directory exists

An empty `~/.agents/hooks/` still means the user adopted the family, so
leftovers are reported. No directory at all means the family is unused, and no
agent's own hooks are listed. Without that distinction, either a hook deleted
from the Store would go unreported (the ticket requires reporting it), or every
user who never touched hooks would see every tool's hooks listed as Foreign.

## 2026-08-13 — A Foreign hook is named by its program, not its command line

Reports name our own hooks by their Store command, unresolved. A Foreign hook
is named by its program only: `curl …` rather than the full line. Another
tool's command line can carry a credential, and agentstow echoing it into a
terminal, a CI log or `--json` would be exposure the user never asked for —
found by probing a hook with an `Authorization:` header on it.

## 2026-08-13 — Hook commands are never expanded, because the command is the identity

`${env:VAR}` is written through verbatim rather than resolved. Element identity
matches hooks by command string, so a resolved command stops matching the
reference that produced it: the hook was reported `missing` and its own
rendering `foreign`, on every run, with `status` stuck at exit 2 and `sync`
claiming a change forever. Verified by reproduction, then by the fix converging
across three different values of the same variable.

The consequence is better than the bug it replaces: a hook that needs a secret
reads it from the environment when it runs, so the value never lands in a config
file at all. This is the opposite of the MCP rule, and deliberately so — there
the value is data an agent consumes, here it is a name agentstow matches on.

## 2026-08-13 — Gemini hook timeout units are unresolved

Claude and Codex hook timeouts are seconds. Gemini's may be milliseconds: on
this machine claude-mem writes `10` to Claude and `10000` to Gemini for what is
plainly the same ten seconds, which only makes sense as a unit conversion. But
plannotator writes `345600` to both Codex and Gemini, which is four days as
seconds and under six minutes as milliseconds — so the evidence conflicts, and
gemini-cli is not installed here to settle it.

agentstow therefore writes the timeout **verbatim** and does not guess. Applying
a 1000x multiplier on this evidence risks a hook that is killed instantly; not
applying one risks a hook that hangs. Passing through what the user wrote is the
only choice that is wrong in a way they can see and correct. Resolve this by
reading gemini-cli's source when it is available.

## 2026-08-13 — Two Gemini event mappings left deliberately unmapped

A design review proposed mapping `UserPromptSubmit` to Gemini's `BeforeAgent`.
Gemini's own config uses `BeforeAgent` for claude-mem's *session-init*, which
reads more like session start than prompt submission, so the mapping is not
clearly right. An unsupported event is reported out loud; a wrong mapping is
silent. The conservative choice stands until Gemini's event semantics can be
confirmed. The same applies to the claim that Codex has no `Notification` event.

## 2026-08-13 — The matcher is part of what "managed" means for a hook

Drift was decided by comparing the rendered leaf — type, command, timeout —
but a hook's matcher lives on the enclosing *group*. So changing a Store hook's
matcher from `Bash` to `Write` reported "Everything is up to date" and left the
hook firing on `Bash`: a narrowing safety constraint the user had deliberately
changed, silently ignored. The comparison now includes the group's matcher
(absent and empty treated alike), and applying a change moves the hook into a
group with the right matcher rather than editing in place.

Moving removes only leaves whose command matches ours, and drops a group only
when emptying it left it with nothing — so a foreign hook sharing our old group
stays exactly where its owner put it.

## 2026-08-13 — `init` deliberately does not create `hooks/`

The other families' directories are created by `init` because an empty one is
inert. An empty `hooks/` is not: the family treats directory presence as "the
user adopted this", which is what makes a hook deleted from the Store report as
a leftover. Creating it for everyone would show every user every other tool's
hooks as Foreign. `doctor` reports the family as `absent` instead, and
`hooks/<Event>.toml` is created by the user when they want it.

`doctor` now also warns about non-`.toml` files in `hooks/`, which the family
would otherwise skip in silence — the exact failure the Store hygiene scan was
introduced to end.

## 2026-08-13 — The Marker is what makes pruning safe

Rendered files are the only family agentstow deletes without being asked by
name. That is sound precisely because the Marker proves authorship: a file
carrying it was generated here, so removing it when its Store entry goes is
returning the machine to a state agentstow created. A file without one was
written by somebody else and is never touched — verified against the two real
`plannotator-*.toml` commands on this machine, which survive a sync untouched.

This is not in tension with the MCP rule against markers (ADR-0003). There a
marker would have had to live inside data the agent parses as configuration;
here it is a comment in a file agentstow generates wholesale, invisible to the
agent reading it.

## 2026-08-13 — Rendered prompts use escaped strings, not triple quotes

Gemini's own command files use `"""..."""` for multi-line prompts. agentstow
emits an escaped basic string instead, which is valid TOML carrying identical
content — verified by round-tripping a body containing `"""`, backslashes and
non-ASCII. Matching Gemini's cosmetic style would mean hand-rolling the escaping
of triple quotes inside triple quotes, which is exactly the kind of thing that
produces an unparseable file for one unlucky user. The Marker already tells a
reader to edit the Store rather than the file.

## 2026-08-13 — The first-run report answers "what of mine could this take over"

On an empty machine the interesting question is what agentstow does; on a used
one it is what of the user's existing config the Store could hold. So `init`
reports agents found, then per-family adoption candidates with the command that
would take them, then conflicts with their remedy.

A candidate is a *real* object in a Target directory whose name the Store does
not already hold. A symlink is never a candidate: it is either already ours or
Foreign, and `adopt` refuses both. On this machine that correctly surfaces the
three hand-made `plannotator-*` skill directories out of the 160 entries in
`~/.claude/skills`, the rest of which are links.

Long lists are capped at eight entries with a count of the remainder — a first
run on a populated machine should be readable, and the full picture is what
`status` is for.

## 2026-08-13 — Packaging is verified by a script, not at the CLI seam

Every other family is tested by driving `agentstow::run` against a fixture tree.
Packaging cannot be: the thing under test is what npm and cargo do with the
built artefact, which lives outside the process. `scripts/verify-packaging.sh`
is therefore its test — it builds, assembles, packs, installs offline from local
tarballs, runs the installed launcher, checks exit-code propagation and the
no-platform-package message, and dry-runs every publish.

One packaging fact *is* testable at the seam and now is: `--version` must report
the crate version, because `build-npm.sh` reads that same version out of
`Cargo.toml` and stamps it onto all five npm packages.

## 2026-08-13 — A dry-run publish does not prove the scope is claimable

`npm publish --dry-run` on `@agentstow/darwin-arm64` reports `ok` today, even
though `npm view @agentstow/darwin-arm64` returns 404 and `npm org ls agentstow`
returns 403. The dry run packs and validates locally; it never asks whether the
scope exists or whether you may publish into it. Treating a green dry run as
release-readiness would fail at the worst moment, so the runbook says outright
which checks actually settle it.

The verification script classifies registry outcomes rather than conflating
them: "version already published" and "scope not claimed" are expected states
before the first release and are reported with a pointer to the runbook, while
any other failure is still a hard error.

## 2026-08-13 — The published version is left at 0.0.1 deliberately

`agentstow@0.0.1` is the name-claim placeholder and cannot be republished, so a
real release must bump it. Which number that is — 0.0.2, 0.1.0, 1.0.0 — says
something about the project's maturity that is Frank's to say, not a detail to
settle in passing. `build-npm.sh` reads the version from `Cargo.toml` and stamps
it everywhere, so bumping one line is the whole change.

## 2026-08-13 — Platform packages are published before the launcher

The launcher declares the platform packages as optional dependencies. Publishing
it first leaves a window in which `npm install agentstow` resolves the launcher,
finds no platform package, and installs a command that cannot run. The runbook
fixes the order and says why.

## 2026-08-13 — Rust edition 2024, MSRV 1.97, version 1.0.0

Frank claimed the `@agentstow` npm organisation and set the version to `1.0.0`,
which unblocked the last of ticket 14. The crate then moved to edition 2024 with
`rust-version = "1.97"`, the current stable at the time.

The MSRV is deliberately the latest rather than the oldest that compiles. Almost
everyone gets agentstow as a prebuilt binary through npm, where the toolchain is
irrelevant; only `cargo install` users are affected, and they are the audience
most likely to be current. The release workflow now installs and selects stable
explicitly, because a runner's preinstalled toolchain could otherwise be older
than the declared edition and fail with a confusing message.

Edition 2024 turned three nested `if let` blocks into clippy warnings, which
let-chains — stabilised in the same edition — resolve directly. The suite is
unchanged at 238 tests and clippy is clean again.

## 2026-08-13 — `npm org ls` returning 403 is not evidence

After the organisation was created, `npm org ls agentstow` still returned 403
from this machine. The publishing token can publish but is not authorised to
read organisation membership, so a 403 there says nothing either way. The
runbook records this so a future release is not held up by a check that cannot
answer the question — the publish itself is what settles it.

## 2026-08-13 — Both macOS targets build on Apple silicon runners

The first tagged run exposed a real hole: `build darwin-x64` asked for
`macos-13`, an image GitHub has retired, so the job sat queued with no runner
assigned while the other four finished in under a minute. It would have hung
until timeout, and the packaging job — which needs all four binaries — would
never have started. A workflow that has never run is not a verified workflow,
which is the lesson worth keeping.

Both macOS targets now build on `macos-latest`. rustc cross-compiles
`x86_64-apple-darwin` from Apple silicon using the SDK already on the runner,
so nothing else is needed, and a single label cannot be retired out from under
one architecture while leaving the other working.

## 2026-08-13 — The deploy token is scoped to one zone, not all three

`CF_DEPLOY_TOKEN` carries Workers Scripts: Edit on the account, plus Workers
Routes: Edit and Zone: Read on `agentstow.dev` alone. It cannot see or change
`agentstow.com` and `agentstow.org`, which is a deliberate narrowing rather than
an oversight: those two zones hold only the canonical-host redirect rulesets,
those rules are static, and nothing in the deploy path rewrites them. Granting
CI the ruleset scope on all three zones would widen a secret that any workflow
on the repo can read, in exchange for an ability the workflow never exercises.

The `max-permissions-cli` token in `~/.zshenv` was not reused for the same
reason — full account access should not be reachable from CI.

The scopes are confirmed sufficient by run `31746869454` deploying green, not by
reading the table. An under-scoped token would still pass the workflow's
`refuse to deploy without a token` guard, which only checks the secret is
non-empty; it would fail later inside `wrangler deploy`. The guard proves
presence, the run proves permission.

## 2026-08-13 — The Store is a commons agentstow rents, not one it owns

`~/.agents/` was chosen as a neutral name; it has since become an interop
contract. opencode and oh-my-pi scan it natively, Hermes Agent ships
`~/.agents/skills` as the documented example for its `skills.external_dirs`
setting, and the `skills` CLI keeps `.skill-lock.json` in the root. Renaming it
to `~/.agentstow/`, or moving it under XDG, would break tools that hardcode it —
so the path is now fixed by other people's source, not by preference. ADR-0004
records this along with the split from `~/.agentstow/`, whose real justification
is the git boundary (the Store is committed and synced; the lock file and
per-machine tweaks must not be), not the "purity" argument ADR-0002 gave.

The consequence is that agentstow must admit tenancy. `doctor` now names
Store-root entries that are not its own families, and deliberately names
*entries* rather than counting *tools*: filenames carry no authorship, so one
neighbour may own three files and agentstow cannot tell. `status` stays silent —
a co-tenant has no fan-out relationship to report — and `init` was left alone.

## 2026-08-13 — Hermes does follow symlinks; the premise was three months stale

The plan to give Hermes real copies rested on the belief that it cannot follow
symlinked skills. That was true and is documented — NousResearch/hermes-agent
issue #8293, where `rglob("SKILL.md")` and `os.walk()` skip directory symlinks
on Python 3.11+ — but it was fixed on main in April 2026, and the installed
v0.20.0 walks with `followlinks=True`. The 140 symlinks already in
`~/.hermes/skills`, the oldest from May, had been working the whole time.

So no `Skills::Copy` mechanism was built. That matters beyond Hermes: a copy
family would have needed marker identity (ADR-0003) applied to a *directory*,
and the marker is a first-line comment in a file — there is nowhere to put one
in a skill directory that the agent will not also read as content. ADR-0001's
"drift is impossible by construction" survives intact.

Hermes is registered as `Skills::FanOut` rather than `Skills::Native` even
though it *can* read the Store directly, because that requires the user to set
`skills.external_dirs` and agentstow does not write that file. A registry row
states what is true unconditionally; a `Native` claim would be false on an
unconfigured machine, and `doctor` would report a capability the agent does not
have.
## 2026-08-13 — claude-mem's instruction files were moved aside, not merged

The first real `sync` on the author's machine hit both conflict paths at once:
`~/.config/opencode/AGENTS.md` and `~/.gemini/GEMINI.md` carried the
`<claude-mem-context>` marker, so agentstow refused them and asked for a move
or a merge. Merge was rejected because the content was not instructions — both
files held only claude-mem's first-run placeholder ("no memory yet"), never
updated since June and July despite daily use, and folding a per-agent memory
block into the Store's `AGENTS.md` would fan it out to every agent. The files
were renamed to `*.claude-mem.bak` in place and sync created the symlinks.

The residual risk is recorded rather than solved: claude-mem's opencode and
Antigravity installers write these paths with a plain `writeFileSync`, which
follows symlinks. Both writers run only on explicit install commands and are
guarded by the marker check, but a re-install would append the placeholder
*through* the link into `~/.agents/AGENTS.md` — and the Store is not currently
a git repo, so nothing would flag it. Two follow-ups fall out: the Store ought
to be under git (its own pitch says "committable"), and a conflict message that
names the owning tool could also warn that the owner may write through the
future symlink.

## 2026-08-13 — variant-identical counts against sync, but sync will not fix it

After the full sync, `status` still exited 2 for exactly one item: openclaw's
`agent-reach`, a real directory byte-identical to the Store — a stray copy from
a direct install, not a variant. `sync` deliberately leaves variant-identical
entries alone (conservatism is right: collapsing a directory is a delete), yet
`status` counts the item and its footer says "run `agentstow sync`", which is
advice that cannot succeed. The copy was verified identical with `diff -r`,
removed by hand, and re-linked by sync; exit code went to 0. The UX gap stands
as product feedback: either variant-identical should not count toward the exit
code, or the footer should name the manual collapse instead of pointing at
sync.

## 2026-08-14 — The Chinese pages say zh-Hans, except where Facebook is asking

The Simplified Chinese mirror targets a script, not a country — readers in
Singapore and Malaysia are not `zh-CN`. So `<html lang>`, hreflang, the
sitemap alternates and `Content-Language` all say `zh-Hans`, and CSS scopes
with `:lang(zh)`, which matches it by prefix. The one exception is
`og:locale`, which takes Facebook's enumeration and gets `zh_CN`.

## 2026-08-14 — /zh is public/zh.html, not public/zh/index.html

Wrangler's default `html_handling = "auto-trailing-slash"` would serve
`zh/index.html` canonically at `/zh/` and answer `/zh` with a redirect hop.
A flat `zh.html` serves at exactly `/zh` — matching how `docs.html` already
serves at `/docs` — so the hreflang URLs carry no trailing-slash asymmetry.
The Chinese docs and 404 live under `public/zh/` because they need the path
prefix: `zh/404.html` is what makes a miss under `/zh/*` serve the Chinese
404 via nearest-404 walk-up, which CI now proves after every deploy.

## 2026-08-14 — The zh pages share the English og.png

The OG artwork is the mark, the wordmark, and English-by-design tokens; a
translated card would mean a second screenshot artifact to regenerate on
every palette change, for a marginal gain. The zh pages get translated
`og:title`/`og:description` and `og:locale zh_CN` over the shared image.
Revisit if the zh pages grow their own social traffic.

## 2026-08-14 — What stays English on the Chinese pages

CLI commands, verbatim CLI output, status tokens (`linked` … `conflict`) and
the capability-matrix mechanism tokens stay English everywhere, because the
tables must match what the tool actually prints. Authored commentary is
translated even inside `<pre>`: `<span class="c">` comment lines and the
annotations in illustrative trees ("your instructions, once"), which no real
`ls` ever printed. Vocabulary terms render as 中文（English）— the English
token is the thing `status` emits, so it stays visible. Section ids are
byte-identical across languages so `#fragment` deep links work in both.

## 2026-08-14 — The site is no longer zero-JavaScript, by exactly one file

The user asked for automatic browser-language detection, and the config
deliberately has no Worker (every request a free static-asset hit), so the
detection is client-side: `public/lang.js`, ~30 dependency-free lines. CSP
moves from `script-src 'none'` to `'self'` — still no inline and no
third-party script, and the site works with JS disabled. The script
negotiates over the full `navigator.languages` list, redirects only
Simplified-script tags (`zh`, `zh-Hans*`, `zh-CN`, `zh-SG`, `zh-MY`) from
`/` and `/docs` to their twins, stores an explicit switcher choice in
localStorage as the override, and never redirects away from a `/zh` URL
someone opened on purpose. Traditional-script readers keep English rather
than being force-fed 简体. The rejected alternative — a Worker with
`run_worker_first` on the hot paths — would have contradicted wrangler.toml's
documented no-Worker stance and put the two busiest routes on paid
invocations.

## 2026-08-15 — CI publishes releases, authenticated by OIDC, not tokens

The runbook's founding rule was "CI builds and verifies; it never publishes."
Frank reversed that for 1.1.2: a `vX.Y.Z` tag push now publishes to crates.io
and npm from the release workflow. The unsettled call was the credential. A
stored npm token was rejected on the runbook's own evidence — the account
enforces 2FA, classic automation tokens are gone, and granular tokens that
bypass 2FA are restricted from January 2027 — so both registries authenticate
by OIDC trusted publishing: per-run credentials, nothing stored in repo
secrets, provenance attestations for free. Costs accepted: a one-time
per-package trusted-publisher configuration on each registry from the owning
account, and a hard dependency on npm ≥ 11.5.1 in the publish job (asserted,
not assumed). Publish jobs run only on tag pushes, a guard job refuses a tag
that disagrees with `Cargo.toml`, and both publishers skip versions the
registry already has, so a re-run after a partial release converges instead
of tripping on publish-over-existing errors. The manual OTP path survives in
the runbook as the fallback.

## 2026-08-15 — 1.1.2's npm binaries shipped non-executable; 1.1.3 is the fix

The first CI-published release passed every check and still shipped broken:
`actions/download-artifact` strips file modes, so the publish job packed the
platform binaries at 644, and since the raw binary is not a declared npm `bin`
entry, install-time repair never happens — every `npm install agentstow@1.1.2`
fails with EACCES at spawn. The package job's own offline-install test could
not catch it because it runs on the pre-upload `dist/`. Fixed twice over in
1.1.3: the publish job re-chmods after download and proves it by executing the
linux-x64 binary on the runner, and the launcher now repairs an EACCES spawn
by chmodding 0o755 and retrying once. The runbook records the artifact
permission trap. crates.io 1.1.2 is unaffected; the npm 1.1.2 packages should
be deprecated once 1.1.3 is up.

## 2026-08-15 — Windows support: the shape of the port

Adding Windows (1.2.0) forced five calls the spec never made. Symlink creation
now lives in one helper, `link::create_symlink`: Windows links are typed, so
the flavour is read from what the link text resolves to at creation time, a
dangling target defaulting to a file link; privilege failure (os error 1314)
is rewritten into "enable Developer Mode or elevate". The global lock keeps
flock on Unix and becomes an empty-share-mode open on Windows — same
holder-until-exit semantics, no new dependency. File modes are Unix-only:
`write::atomic` reports mode 0 on Windows and `is_exposed` is always false
there, because inherited NTFS ACLs are the actual privacy story and any bit
we invented would make the secrets warning a lie. `doctor`'s writability
check, `access(2)` on Unix, becomes a create-and-remove probe file on
Windows — the one sanctioned exception to doctor being read-only. The
integration tests stay Unix-only; release builds never compile them, and CI
has no Windows test leg yet. win32-arm64 is cross-compiled on the x64 image
and is the only target whose binary CI never executes. The first publish of
the two new npm packages cannot use trusted publishing (npm only trusts
publishers of packages that exist), so bootstrapping them is a one-time
manual publish before v1.2.0 can be tagged.

## 2026-08-15 — Site deploys moved from GitHub Actions to Workers Builds

The 2026-08-13 decision to keep site deploys in Actions is reversed, by human
call, to converge with openroutine: both repos now deploy `site/` through
Cloudflare's Workers Builds GitHub App (root `site`, watch path `site/*`,
previews on non-production branches). `deploy-site.yml` is deleted, the
`CF_DEPLOY_TOKEN` repo secret removed, and the Cloudflare-side token revoked;
the workflow's smoke checks moved into site/DEPLOY.md §Verifying. The
2026-08-13 finding that connecting is dashboard-only still holds — the
connection was made by hand in the dashboard. Commit 5a817e7.

## 2026-08-16 — The Store is renamed the Commons

Settled after three naming sessions ("canonical directory", then
Store/Commons/DotAgents/Canonical head-to-heads) with the neighbors' source
read for evidence. Neither adjacent tool uses "Store": iannuttall/dotagents
says `canonicalRoot` in code and "canonical .agents folder" in prose;
agentsync says "canonical source" (from chezmoi), package `internal/source`,
struct `Canonical`. Their shared noun "source" is a three-way homonym — the
central dir (agentsync), the canonical end of one link (dotagents), and the
*outside* home of a Sourced entry (here) — which retroactively justifies this
glossary's ban on "source"/"canonical source" for the central dir.
"Canonical" itself fails as a name: it is an adjective, decays into "the
canonical directory" in prose, and the output-vocabulary test could not
distinguish the term from ordinary adjectival use. "DotAgents" is five other
projects' proper name. **Commons** won on the merits: it is ADR-0004's own
word promoted to term of art, it names the fact most at risk of being
forgotten (shared-not-owned; both neighbors assume they own their directory —
co-tenancy is agentstow's actual differentiator), it compounds cleanly, and
it dissolves the Store/Sourced phonetic rub. Costs accepted: ~387 code
occurrences, ~38 message lines, the docs sweep, and one-line amendment notes
on ADR-0001..0005, whose historical wording stays as written. The word
"canonical" remains free for ordinary description — the README gloss is "the
Commons — the canonical `~/.agents/` directory". Sourced/Source keeps its
name despite the ecosystem homonym; its glossary entry now says so.

## 2026-08-16 — Private files move to XDG; ~/.agentstow is retired (hard cut)

This reverses, by human call, ADR-0004's third settled question, which
rejected the XDG split as "trading one dotdir for two." The new weighting:
users version `~/.config` in dotfiles repos but not machine-generated state,
and state should be wipeable without losing configuration — so config goes to
`$XDG_CONFIG_HOME/agentstow/` (default `~/.config/agentstow/`) and the lock,
agentstow's entire state, to `$XDG_STATE_HOME/agentstow/` (default
`~/.local/state/agentstow/`). The lock is a runtime file by XDG purism, but
`XDG_RUNTIME_DIR` is routinely unset on macOS; the state dir is the
deliberate simplification. Resolution is XDG-style on every platform
including Windows (explicit `XDG_*` if set, else derived from the overridable
home) — git's precedent, zero new dependencies, no new env vars. The cut is
hard: the legacy `~/.agentstow/` is ignored, `doctor` names a stray one and
states the move, and the known hazard (a silently ignored legacy config
re-enables disabled targets) was accepted with eyes open over a fallback-read
window. The Commons itself never moves — ADR-0004's "a commons that resolves
differently per tool is not a commons" reasoning is untouched. Because the
old config location stops working, v2 ships as 2.0.0.

## 2026-08-16 — The .agents Protocol is adopted with two reservations

The ".agents Protocol" (dotagentsprotocol.com, draft 2026-02-24) was
verified: real, one author, one implementing app (his own), spec dormant
since March, no license file, casing that conflicts with the installed base.
What is independently real is narrower: `.agents/skills/<name>/SKILL.md`
(uppercase) is natively read by opencode at user level and by
Codex/Cursor/Cline/Amp/Gemini CLI/Copilot at project level — which the
Commons already implements byte-for-byte. Full adoption was on the table and
declined for two reasons that became the recorded reservations: **casing**
(lowercase `agents.md`/`skill.md` would break skills on every case-sensitive
system for every mainstream agent, and would desert the real `AGENTS.md`
standard — where the draft and the installed base disagree, the installed
base wins) and **secrets** (`models.json` carries provider keys, which do not
belong in a committed commons — ADR-0002's posture). What is adopted:
`subagents/` → `agents/` (the protocol's name, also matching both fan-out
destinations; `subagents` survives as a config-key alias and `doctor` hints
when a non-empty old dir exists), and the protocol's surfaces (`tasks/`,
`memories/`, `models.json`, `system-prompt.md`) become named Protocol
surfaces in `doctor` — attributed, never touched, a family only when a real
consumer exists. `mcp.json` already conforms. Revisit on evidence of real
adoption.

## 2026-08-16 — Two verbs join the surface: revert, and mcp enable/disable

**revert <agent>** exists because disabling a target orphans its fanned-out
links forever — sync never visits again, no state remembers it was managed,
and `mcp remove` skips it — and hand-editing merged config files is exactly
the surgery `mcp remove` was created to avoid. It removes everything
agentstow put into one target (links, merged MCP/hook keys, Markers) and
**refuses unless the target is already disabled**, printing the exact
`targets.<name> = false` line: the forced order makes the intent durable in
config before the teardown, so a later sync cannot silently redo what revert
undid, and revert never edits the user's config as a side effect.
**mcp enable|disable** exists because JSON has no comments: parking a server
means deleting and retyping its definition. The state is `"disabled": true`
on the entry in the Commons `mcp.json` — the file remains the interface, the
verbs are sugar, the property travels with the definition across machines.
Disable acts immediately under the lock like `mcp remove` (managed entries
removed everywhere, definition and rules kept); a disabled server renders
nowhere, lists as `(disabled)`, and counts as clean.

## 2026-08-16 — What v2 deliberately does not adopt from its neighbors

GNU Stow (manual-verified, 2.4.1) and agentsync were enumerated end to end;
the adoptions are two-phase sync (ADR-0007), `--dry-run` on adopt, revert,
enable/disable, and an output-vocabulary test that enforces CONTEXT.md's
avoid-lists in user-facing strings. The refusals, each on its own grounds: no
`remove <entry>` verb (`rm` + sync's prune is the interface; `mcp remove`
stays the exception because its entries hide in shared rendered files); no
target enable/disable verb (the config file is the interface; revert's
refusal prints the line); no bulk `--all` path adopt (init's report + a shell
loop; one Variant refusal mid-batch would reopen partial-failure semantics
kept deliberately atomic); no color, ever (plain text and the stdout/stderr
split are the interface — also the simplest NO_COLOR compliance); no
verbosity levels (a mysterious decision wants a better report line, not a
trace flag); no `--ignore/--defer/--override` patterns (Variants, disabling,
and Co-tenancy are the structural answers; pattern flags are a model
admitting it does not fit); no CLI flags for the env vars (redirecting the
tool is a machine-level fact — the vars are promoted to documented interface
instead); `sync --dry-run` keeps exit 0 (status is the drift gate — one
command per question); no `explain` (doctor + status + the printed plan); no
LSP family (no consumer in the registry — same bar as the protocol
surfaces); no undo/backups (an undo journal is state, and ADR-0001 says no
state file, permanently — refusals-before-writes, two-phase abort-clean,
dry-run, idempotent re-runs, and a git-versioned Commons are the undo
story); and project scope (`./.agents/`) is a reasoned non-goal — agents
already read project-level `.agents/skills` natively, so a project agentstow
would manage a directory its consumers handle themselves; the one holdout
(Claude Code reads `.claude/skills/`) is a per-project symlink away. One
model correction did come out of the review: registry rows flip
`Skills::FanOut` → `Native` where an agent's own docs show unconditional
user-level `~/.agents/skills` reading, because fan-out into a native reader
produces duplicate skills.

## 2026-08-16 — Two-phase sync: a skipped agent config still does not block

Ticket 04 (ADR-0007) says any survey problem means "exit 1, nothing written".
Interpreted "survey problem" as a fault in the *Commons* — an unparseable
`mcp.json` or hook file, an unset `${env:VAR}`, an unreadable command file —
because those make the plan itself unbuildable; they are now collected across
every family and any one of them stops the run before its first write. An
unparseable *agent* config keeps the 2026-08-13 decision unchanged: that
Target is skipped and reported (exit 1) while every healthy Target still
syncs. ADR-0007's named failure mode is a Commons fault leaving a half-synced
machine; one third-party file agentstow promised never to touch denying
service to every other agent is the failure mode the earlier decision exists
to prevent, and three tests pin it. Consequences: `status` and `mcp list`
refuse on a Commons fault naming *all* faults, not just the first.

## 2026-08-16 — Adopt fans instructions out too, and previews it from the origin

Ticket 05 left the instructions-file absorb's fan-out optional ("only insofar
as instructions sync applies... if awkward, keep link-at-origin only"). It is
not awkward: adopt reuses `instructions::survey` + `apply` verbatim — the same
per-agent mechanisms sync runs, origin skipped since it already holds its
link — so an adopted AGENTS.md lands its import line in Claude's file and its
symlinks everywhere else during the adopt itself, and the no-op-sync property
holds for instructions exactly as for the symlink families. One wrinkle that
survey guards on the Commons file existing, which a dry run of a first-time
absorb has not created yet: the preview surveys against the *origin* file
instead — same content, consulted for nothing beyond existing — rather than
growing the survey a bypass flag or letting the preview go silent. Accepted
drift: a hand-made link already pointing at the not-yet-existing Commons file
previews as foreign; a real run classifies it linked. Two smaller calls in the
same slice: a fan-out write failure is reported and the remaining agents still
get their links (exit 1), mirroring sync's write-failure semantics rather than
aborting a half-fanned adopt; and Variants or Foreign links met during the
fan-out are named in the report using `link::State`'s own label/note words, so
adopt and sync describe the same state in the same vocabulary.

## 2026-08-16 — Source mechanic: what counts as Sourced, and how "same place" is judged

Three calls ticket 06 left open. First, doctor's `(N sourced)` suffix counts
every symlink entry in a family directory — dangling ones included — so it can
exceed the scan's entry count (`skills 0 (1 sourced)` for a lone dangling
link). The suffix and the `Sourced entries:` section describe the same set,
because the section exists to answer "what must I clone?" and a not-yet-cloned
source is exactly the interesting row. Second, any family-entry symlink counts
as Sourced in doctor regardless of where it resolves, per CONTEXT.md: Sourced
entries are the only ones recognizable on disk, and they are recognizable *by
being symlinks* — doctor does not second-guess where a hand-made link points.
Third, the idempotence comparison (`same_place`) answers lexically first —
normalized link resolution equals the normalized input, so dangling paths
still compare — and falls back to `fs::canonicalize` on both sides, so an
absolute link reaching the same file by another spelling still reads as "already
Sourced". A canonicalize-only rule would break for sources that do not resolve;
a lexical-only rule would refuse a repoint that is not one. Smaller notes: the
git walk-up tests the path itself and every ancestor (a repo root is itself
adoptable) via `symlink_metadata`, so `.git` as file or directory both count;
and the dry-run family fan-out surveys against the not-yet-created Commons
destination, accepting the same preview drift ticket 05 logged for
instructions.

## 2026-08-16 — Copy mechanic: absorb joins the link matrix, and the shape refusal's converse

Three calls ticket 07 left open. First, the unified matrix's symlink cases now
answer in *absorb* too, not only source and copy: the ticket's sweep demands
all four collision cases behave identically across all three mechanics, and
absorb previously compared contents straight through a Commons symlink — for a
link resolving back to the input path, "identical" would have authorized
deleting the only real copy. The symlink answer therefore precedes any content
comparison in every mechanic, via one shared helper (`existing_link_answer`),
so the no-op and repoint wordings are byte-identical everywhere; for an
instructions-surface absorb the entry is labeled `AGENTS.md`. Second, the
ticket gave only the directory-family shape refusal ("skills entries are
directories — this is a file"); the Markdown converse is worded in the same
voice as "commands entries are single .md files — this is a directory", taking
"single .md file" from the ticket's own shape gloss. The check follows the
symlink (`is_dir` through the link), matching copy-through semantics; a
dangling symlink input under a directory family reads as "this is a file" —
accepted as a rare approximation rather than a fourth wording. Third, a failed
copy removes whatever half-copied destination it left, so an interrupted
adopt never leaves a partial Commons entry for the next sync to fan out —
mirroring absorb's put-it-back recovery in spirit, though here the original
was never touched.

## 2026-08-16 — revert <agent>: the calls ticket 08 left open

Four. First, `targets.<name> = false` now switches a config-defined target
off in `target::resolve` exactly as it does a built-in one — previously the
key was read and then silently ignored for customs. Revert's refusal prints
that line as the guarantee a later sync will not redo the teardown, so the
line has to mean what it says for every target; a test pins a disabled
custom target staying reverted. Second, hook removal uses the survey's
identity, not the command string alone: an element is ours when its command
matches a Commons declaration *under that declaration's native event for
this agent*, so a user's own reuse of the same command under another event
survives. An event array the removal empties loses its key too — every
group in it was one ours had emptied, or it would not be empty — while an
array that was already empty is never entered, and the root `hooks` key
stays even at `{}`, because nothing proves agentstow created it. Third,
removing the import line removes exactly the line: a CLAUDE.md that held
nothing else is left behind empty rather than deleted — the file is the
user's, and the one sanctioned edit gets one equally narrow undo. Fourth,
the MCP sweep ignores allowlists: any entry in this agent's config whose
name is in the Commons is Managed wherever it appears, so a rule scoping
the server elsewhere does not shield a stale copy from the teardown.

## 2026-08-16 — Sync removes a Disabled server's renders; never-delete stands

The 2026-08-13 rule — `sync` never deletes an MCP server — exists because,
without state, a name that has vanished from the Commons is indistinguishable
from one the user added by hand, and deleting on that evidence would
eventually delete someone's hand-added server. A Disabled server presents the
opposite evidence: its name has not vanished. It is right there in the
Commons, carrying `"disabled": true`, so an entry under that name in an
agent's config is Managed under exactly the name identity every write relies
on, and the Commons file itself says to render it nowhere. Removal on sync is
therefore name-identity-safe — `sync` still deletes nothing whose name the
Commons does not claim — and it is what "renders nowhere" has to mean once a
hand-set key (the file is the interface) can disable a server that
yesterday's sync rendered. Without it, hand-disabling would strand renders
forever: `status` calling the machine clean while every agent still runs the
server. So a name absent from the Commons is never deleted (that remains
`mcp remove`'s job, by explicit request), while a name present and marked
disabled has its renders removed by name through the same `strip` primitive
`mcp remove` uses; until then `status` counts each leftover as actionable.

## 2026-08-16 — Native flips for codex, cursor and gemini; legacy dirs prune

Three registry rows flip on primary-doc evidence, each verified 2026-08-16
against the vendor's own documentation and recorded in the row comment. Codex:
the official skills doc (https://developers.openai.com/codex/skills) lists
user-level `$HOME/.agents/skills` unconditionally, and the shipping source
(codex-rs host_roots.rs) confirms it — while still reading `$CODEX_HOME/skills`
as a "deprecated user skills location, kept for backward compat". Cursor:
https://cursor.com/docs/context/skills says skills are loaded automatically
from `~/.agents/skills/`, plus the legacy-compat `~/.cursor/skills/`,
`~/.claude/skills/` and `~/.codex/skills/` — the worst duplication of all.
Gemini CLI: user skills come from `~/.gemini/skills/` or the
`~/.agents/skills/` alias with `skills.enabled` defaulting true
(https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/skills.md), so
its row moves from None straight to Native; we never fanned skills out to it,
so there is nothing to clean. Cline stays `Skills::None`: its docs
(https://docs.cline.bot/customization/skills) list no `.agents` path at any
level, support exists only in undocumented code that silently regressed once
before, and skills launched behind an opt-in toggle — below ADR-0004's bar
that a registry row must be true unconditionally. Hermes likewise stays
FanOut for the reason its row already records.

The stranded-links consequence needed more than a one-time cleanup note,
because for codex and cursor the old fan-out dirs are not litter: both agents
still read them beside the Commons, so every link of ours there is a live
duplicate — the same skill seen twice. The registry therefore has to express
"Native, with a legacy dir to clean": `Skills::Native` gains a
`legacy: Option<&'static str>` field, and sync visits a legacy dir in
prune-only mode — it removes links of ours (resolving into the Commons, the
same `link::classify` identity every command uses), creates nothing, and
touches nothing foreign; Foreign links and real content in a legacy dir are
not even named, since a directory agentstow no longer fans out into is not
its business to narrate. The behavior is stateless and converges: `status`
counts each leftover as actionable (the disabled-server pattern from the MCP
decision above — the Commons still claims the name the link points at) until
a sync prunes it, after which the dir drops out of every report. The prune is
a new `link::State::Duplicate` riding the existing survey/apply machinery, so
sync and status agree by construction; `revert` sweeps the legacy dir too,
because "everything agentstow put into one target" includes what an earlier
registry put; adopt's fan-out skips Native agents as before, flipped ones
included; `doctor` names the arrangement in the capability line — `skills
native (reads the Commons; cleans .codex/skills)`.

Out of scope but recorded: Amp, Copilot CLI and Kimi also read
`~/.agents/skills` per the 2026-08 research, but have no registry rows —
adding agents is a separate change (a data-only row each), and they are the
natural next candidates.

## 2026-08-16 — The vocabulary test enforces what is unambiguous, and no more

Ticket 12's brief — "a test scans every user-facing output string against
CONTEXT.md's avoid-lists and fails on any violation" — is not literally
implementable: "source", "external", "override" and "conflict" are homonyms
whose other senses the tool must say (Source and Conflict are themselves
defined terms), and bare "linked" is the fan-out vocabulary. The test
(`tests/vocabulary.rs`) therefore enforces the word-bounded retired terms
`store` and `subagents` plus the six avoid-phrases with no legitimate other
sense, reads CONTEXT.md at runtime so a glossary change fails the test rather
than being silently ignored, and says in its header exactly what it cannot
check. Three calls within that: the ticket's two anticipated `subagents`
literals (config alias, doctor hint) were three in the code, so all uses were
consolidated into one constant — `Family::PRE_V2_AGENTS_NAME` — and the test
pins its count at exactly one, catching a stale allowlist in either direction;
the checked-in npm launcher `package.json` was bumped to 2.0.0 with its
optionalDependencies pins, although the runbook says only `Cargo.toml` carries
a version (build-npm.sh stamps over it either way), because a checked-in 0.0.1
that never ships is a standing invitation to misread; and the crate/npm
descriptions' "subagents" — outside the test's reach but on every registry
page — was fixed to "agents" in the same sweep.

## 2026-08-16 — README claims corrected to shipped behavior, not the reverse

The docs-vs-behavior pass (ticket 12) treated the code as truth. Three README
sentences were wrong: "every mutating command has `--dry-run`" (only sync and
adopt preview; revert and the mcp verbs are refusal-guarded instead) is now
"`sync` and `adopt` preview with `--dry-run`"; the native-readers sentence
named only opencode and oh-my-pi from before the registry flips, and now names
Codex, opencode, oh-my-pi, Gemini CLI and Cursor for skills plus the legacy
prune; and a Variant "flagged only when identical" is now "counted as
actionable only when identical", since status does list a diverged Variant —
it just never counts it. CLI help gained the three-mechanic long help on
adopt, and doctor's summary line now matches the README's ("installed agents,
Commons hygiene, Sourced entries").

## 2026-08-16 — Site v2 copy: Windows claim reversed, #store anchor renamed

The site-to-v2 pass followed the README as truth beyond the enumerated copy
changes: the pages said "No Windows" / "Windows is not supported", which the
shipped 2.x contradicts, so the install headings now read "macOS, Linux and
Windows" and the warning became the README's Developer-Mode symlink note.
The docs section id `#store` was renamed `#commons` on both languages —
old `#store` deep links break, accepted so no `store` token survives and the
en/zh fragment parity rule holds. "One file seen from ten places" stays: the
v2 README keeps that phrase, so it is not an agent count. The fan-out example
swapped `~/.codex/skills` for `~/.pi/agent/skills` (Codex is Native now), the
landing coverage matrix grew OpenClaw and Hermes rows from `src/registry.rs`,
and zh keeps 子智能体 as prose for the things while every directory reads
`agents/`, with the Commons glossed once per page as Commons（共享配置区）.

## 2026-08-16 — Rename to "dotagents" considered and rejected; name stays agentstow

One day after 2.0.0 shipped, a rename to "dotagents" was weighed (the
dotagents.net and .cc registrations prompted it). Rejected on namespace
evidence, all verified today: npm `dotagents` is a competing tool in this
exact niche ("Carry your AI-agent skills and setup across machines",
beautyfree, v0.4.0), crates.io `dotagents` is squatted with 0.0.0
placeholders since May 2026, the GitHub `dotagents` org exists, and
dotagents.com/.dev/.org are registered to third parties. The name's
mindshare belongs to iannuttall/dotagents (703 stars, same problem space),
with getsentry/dotagents, bgreenwell/dotagents and the ".agents Protocol"
crowding the rest — the v2 naming pass had already rejected "DotAgents" as
a vocabulary term for being an ecosystem homonym, and the same argument
applies with more force to the product name. agentstow, by contrast, is
collision-free on every registry, owns its .dev/.com/.org triple through
2027, and names the mechanic (GNU Stow). dotagents.net and .cc are kept as
defensive holdings and 301-redirect to agentstow.dev via Cloudflare Single
Redirects; no code, package, or site identity changes.

## 2026-08-16 — One tagline everywhere; the hero's "memory" claim removed

Two taglines had drifted onto different surfaces: the site hero (en/zh) said
"Canonical configs (skills, MCP, memory, etc.), fanned out…" while the README
and GitHub description said "One canonical .agents/ folder to power(s) all
your AI agents." The hero's "memory" contradicted the README's explicit "No
memory sync" stance, and the GitHub copy carried a "to powers" typo. Unified
on a hybrid — "One canonical .agents/ folder, fanned out to all your AI
coding agents." — which keeps the "One" promise and the concrete .agents/
anchor, uses the project's own fan-out verb, and narrows the audience to
coding agents. Applied to README, GitHub description, and both site heroes
and titles (zh: 一个规范的 .agents/ 目录，扇出到你所有的 AI 编程智能体);
the longer Cargo/npm descriptions were already accurate and stand.

## 2026-08-16 — npm stays; it is the only prebuilt-binary channel, and npx is now documented

Whether publishing to npm alongside crates.io was still worth its cost came up
today. It is, for a reason stronger than convenience: `release.yml` cross-builds
six targets but uploads them nowhere except `actions/upload-artifact` and the
seven npm packages — there is no `gh release` step and no release asset anywhere
in the repo. npm is not a mirror of a binary distribution, it *is* the binary
distribution. Dropping it would put every user on `cargo install` and a Rust
1.97+ toolchain, which is the opposite of the audience: people already running
Claude Code, Codex and opencode, all npm-distributed. The 2026-08-13 MSRV entry
already assumed exactly this ("almost everyone gets agentstow as a prebuilt
binary through npm"); this makes it explicit rather than implicit. Usage points
the same way and is only weak corroboration at three days old — npm 780
downloads in a week against 86 all-time on crates.io.

The cost side is already paid and mechanised: `Cargo.toml` is the single version
source, publishing is OIDC with no stored tokens, and `verify-packaging.sh` plus
the `package` job test assembly, offline install and launch. crates.io stays too
— three token-free jobs, and it serves `cargo install` users and crate-level
discoverability.

The consequence accepted and not fixed: npm is therefore a single point of
failure for every non-Rust user. Attaching the six already-built binaries to the
GitHub Release would remove that for the price of one step, and is deliberately
left for later rather than folded into this change.

What did change is that `npx agentstow` — supported by the launcher since 1.0.0,
since there is no postinstall and nothing to fetch beyond the binary — was
documented nowhere; the repo's only `npx` mentions were `npx wrangler` in the
site deploy docs. A third line, `npx agentstow doctor`, now sits in all five
install blocks (README, site en/zh home and docs). `doctor` is the right trial
command because it is read-only and branches on `commons.exists()`, so it
reports what a machine has without creating a Commons. Verified cold on
darwin-arm64 against an isolated npm cache before being written down.

## 2026-08-16 — A Homebrew tap in this repo, fed by release assets that did not exist yet

Homebrew has no trusted publishing and never will: the OpenSSF working group
that defines it scopes it to registries that accept author-uploaded builds, and
explicitly names Homebrew as the counter-example because it builds centrally.
So the tokenless property has to come from somewhere else. It does — a tap is a
git repo, and pushing to this one from Actions uses the built-in `GITHUB_TOKEN`,
minted per run and expiring with it. No new secret, consistent with the OIDC
posture on npm and crates.io.

The tap lives in the agentstow repo rather than a separate `homebrew-agentstow`,
which costs one thing: `brew tap agentstow/agentstow` resolves to
`agentstow/homebrew-agentstow` and 404s, so users must tap by URL. Accepted —
a second repo to hold one generated file is worse than one long command, and
the command is in every install block.

This unblocked itself on a prerequisite the 2026-08-16 npm entry had deliberately
left for later: there was no `gh release` step at all, so nothing existed for a
formula to point at. That entry called npm "a single point of failure for every
non-Rust user" and priced the fix at one step. It is now paid — a `release` job
attaches all six already-built binaries plus `SHA256SUMS.txt`, tar for unix and
zip for Windows. Homebrew was the forcing function, but the SPOF is gone for
everyone, including people who never touch brew.

`Formula/agentstow.rb` is generated by `scripts/update-formula.sh`, which reads
the checksums off the published release rather than recomputing them, so the
formula cannot describe an asset that does not exist, and rewrites the file
whole, so it cannot half-update. The `guard`, `release` and `tap` jobs are gated
on the ref rather than the event, so a `workflow_dispatch` at a v\* tag can
rebuild binaries and formula without moving the tag; the registry publish jobs
stay push-only and cannot fire that way.

Two things surfaced only by testing against the real Homebrew 6.0.17 rather
than from memory. `brew audit` rejects a `version` stanza as redundant when the
version is scannable from the asset URL, so the formula omits it. And Homebrew 6
refuses to load a formula from an untrusted third-party tap — `brew trust
agentstow/tap` is a required third line, not a nicety, and install instructions
that omit it simply fail. Verified end to end against the v1.0.0 assets: tap
from a non-`homebrew-*` repo, bare-name resolution, clean audit, download,
checksum, install, and the formula's own test block.

The formula is committed seeded at 1.0.0, the only tag that had assets, and
cannot be bootstrapped to 2.0.1 by dispatching at that tag: `workflow_dispatch
--ref v2.0.1` runs `release.yml` *as of that tag*, which predates these jobs.
Rather than leave the install docs describing a tap a version behind, 2.0.2 is
cut immediately after this — a release whose only content is the pipeline that
produces it, which is also the first end-to-end proof that the `release` and
`tap` jobs work against a real tag rather than against the v1.0.0 assets they
were tested on.

## 2026-08-16 — PyPI as a fourth channel: wheels packaged from the binaries we already build

Asked to publish the CLI to PyPI. There was no Python package to publish —
no `pyproject.toml`, no `setup.py`, not one `.py` file — so this was not a
workflow-and-trust-entry job but a new distribution layer. The name `agentstow`
was free on PyPI, and PyPI's *pending publisher* flow means the project is
created by the first OIDC upload, so unlike npm and crates.io this channel never
needs a bootstrap publish and no token exists at any point.

Two ways to make wheels for a Rust binary. maturin is the conventional answer
and what ruff and uv use, but it compiles the crate itself, so it would have
re-cross-built all six targets in a second matrix and roughly doubled release
wall-clock. Chose instead to package the artifacts the `build` job already
produces, which is the same shape as the npm launcher and the Homebrew formula:
build once, package many. A wheel is a zip with a prescribed layout, so writing
that layout is cheaper than building everything twice. The cost accepted is that
platform tags — `macosx_11_0_arm64`, `manylinux_2_17_x86_64.manylinux2014_x86_64`,
`win_amd64` and the rest — are hand-written, and a wrong tag fails as *no
matching distribution* rather than as anything that names the cause. The `wheels`
job installs the manylinux wheel on the runner to keep at least one tag honest.

The wheels contain no Python. The binary sits in
`agentstow-<version>.data/scripts/`, which pip installs directly onto PATH, so
`pip install agentstow` and `uvx agentstow` reach the same binary as
`npm install -g` with no interpreter shim between.

The bug worth recording, because it is invisible until a user hits it: pip
decides whether to install a file executable with `stat.S_ISREG(mode) and mode &
0o111`. A zip entry written with a bare `0o755` has the execute bits but fails
`S_ISREG`, so pip installed a **non-executable** `agentstow` into the venv's
`bin/` — a command on PATH that cannot run. Nothing in building or validating
the wheel catches it; `twine check` passed, the metadata was right, the zip
looked right, and comparing byte-for-byte against a real maturin wheel was what
located it (`0o100755` vs `0o755`). The fix is `stat.S_IFREG | 0o755`; the
guard is `test -x` after a real `pip install` in CI, not a build-time check.

Also guarded: `build-npm.sh` will fall back to the host binary for *any* target
when the triple's directory is missing, which is harmless there but would seal a
macOS binary into a linux wheel here. `build-wheels.py` restricts that fallback
to the host's own target, because a wheel on PyPI can be yanked but never
replaced.

Deliberately not added: a `pyproject.toml`. Nothing builds from source on the
Python side, and a manifest that does not describe how the artifact is made
would be a decoy for the next person. The channel is documented in the runbook
instead. Trusted publishing reuses `release.yml` and leaves the environment
blank, matching the crates.io and npm entries rather than introducing this
repo's first GitHub environment for one registry.

## 2026-08-16 — A fallback wheel, a tag-locked PyPI environment, and current actions

Three follow-ups to the PyPI channel, taken together because the first two both
touch the wheel pipeline.

**A seventh `py3-none-any` wheel.** `pip install agentstow` on an unsupported
platform failed with pip's generic *no matching distribution found* — no
platform named, no way forward — while the npm launcher for the same case names
what it looked for and points at `cargo install`. Two channels, two standards.
Fixed by shipping a binary-less wheel whose console script prints exactly that
and exits 1. pip ranks any platform tag above `any`, so it can never shadow a
real wheel; that ordering is now asserted rather than assumed, by resolving from
a wheelhouse holding both and checking the binary answers. Rejected the
alternative of an sdist: it would need Rust to build, which is the opposite of
what the wheels are for, and it fails at install time rather than explaining
itself at run time.

**PyPI's environment is now `pypi`, locked to `v*` tags.** The first entry left
it blank to match crates.io and npm, which was defensible but left the trust
entry accepting any run of `release.yml`. PyPI is the one registry that
recommends an environment, and this was the newest entry, so it starts tight
instead. Three things must now agree — the `environment: pypi` key on the job, a
GitHub environment of that name, and the field on PyPI — and the environment
carries a `v*` tag deployment policy, so a branch run cannot reach it even
though it lives in the same workflow file. Sequenced deliberately: the workflow
key and the environment first, while PyPI still accepted `(Any)`, so no step in
the middle could break publishing.

**Actions moved to current majors** (checkout 5→7, download-artifact 4→8,
setup-node 4→7, setup-python 5→7, upload-artifact 4→7). Every run had been
annotating that `upload-artifact@v4` targets the deprecated Node 20, which
trains a maintainer to ignore annotations. Read the major release notes first
rather than bumping blind: the breaking changes are all opt-in parameters
(`archive`, `skip-decompress`, `digest-mismatch`), and default behaviour for
this repo's usage — named upload, download-all-into-a-path — is unchanged.
Validated with a `workflow_dispatch` at main before tagging, which exercises
build, package and wheels while every publish job stays gated off.

Two defects were caught by the repo's own checks rather than in review, which is
the argument for having them. `verify-packaging.sh` picked its wheel with
`ls | head -1`; `py3-none-any` sorts first, so it installed the fallback and
correctly reported that the binary does not run. And packaging silently accepted
a stale `target/release/` from before a version bump, labelling a wheel 2.0.3
around a 2.0.2 binary — harmless in CI, which always builds fresh, but the
runbook's manual-upload path would have put it on PyPI, where a file can be
yanked and never replaced. `build-wheels.py` now runs the host binary's
`--version` and refuses to package on disagreement.

## 2026-09-06 — `rust-toolchain.toml` selects the stable channel; the MSRV stays a version

Three fleet machines failed `cargo install agentstow` today with *requires rustc
1.97 or newer* while their stable channel still sat at 1.96; `rustup update
stable` fixed each one. Frank then asked that the repo itself "use rust stable"
so it stays aligned with that command. Read as a toolchain choice, not an MSRV
change: `rust-toolchain.toml` now names `channel = "stable"`, so a build inside
the checkout uses whatever stable rustup currently has — on a machine whose
default toolchain is something else just as on one where it is already stable,
and the same channel the release workflow installs.

`rust-version` stays `1.97`. The field cannot name a channel, and the 2026-08-13
entry makes it the stable of the release day on purpose; lowering it would
loosen that policy, and raising it to today's 1.98 would only hand the same
failure to more installers. No components are listed: a developer's rustup
installs stable with the default profile, rustfmt and clippy included, while
listing them would make every `--profile minimal` release job download tools it
never runs. The file ships inside the crate like everything else the package
does not exclude, which changes nothing — rustup resolves the toolchain from the
directory the command is run in, never from an extracted crate. The workflow
keeps its explicit `rustup toolchain install stable`: the file would install
stable on a runner that lacked it, but the explicit step names the profile and
prints the version.

## 2026-09-16 — agentstow becomes agent-sync; the published package is `agent-sync-sh`

Frank bought `agent-sync.sh` and asked for the project to be renamed, the site
moved, and the repo transferred to a new org. "stow" was the thing being dropped:
a GNU Stow reference that says nothing to anyone who has not used Stow.

`agent-sync` itself could not be published. crates.io holds an active
`agent-sync` — *"Safely synchronize coding-agent sessions and memories over
SSH"*, v0.7.1, updated nine days ago — and PyPI holds another, *"Bidirectional
synchronization tool for custom agents and settings between Claude Code and
GitHub Copilot"*, v1.2.0. Neither is abandoned, so neither PEP 541 nor crates.io
squatting policy applies. The obvious fallback `agent-sync-cli` is taken too, on
npm, by *"CLI tools to manage synchronization of AI coding agent files, such as
reusable prompts and skills"* — a description of this project. Three separate
occupants in one niche; the name is simply crowded.

A sweep of 48 alternatives found nine clean across crates.io, npm, PyPI and a
free GitHub namespace, but none read better than `agentstow`, and seven had no
`.com`. Rather than take a worse name to win a namespace, the name stays
`agent-sync` where nothing can stop it — the project, the binary, the domain —
and the registries get `agent-sync-sh`, free on all four channels. The suffix is
not arbitrary padding: it is the domain and the org, so `cargo install
agent-sync-sh` reads as "the thing from agent-sync.sh" and, usefully, cannot be
confused with the three rivals. Uniformity across registries was worth more than
the prettier `npm i -g agent-sync`, which was free and is being parked as a stub
only so nobody else can point it at these users.

Splitting the package name from the binary name costs one thing: the binary was
implicit from `package.name`, and now needs an explicit `[[bin]]`, mirrored in
the npm `bin` map, the wheel's `.data/scripts/` placement and the formula's
`bin.install`. Bare `agent-sync` will default to `sync`, because `agent-sync
sync` is the command users would otherwise type every day.

Version restarts at `1.0.0`. The code is at 2.0.5 maturity, but these are new
packages on registries with no history of them, and `3.0.0` as a first release
reads as a mistake.

## 2026-09-16 — The farewell release ships before the org transfer, not after

All nine OIDC trust entries — seven npm packages, crates.io, PyPI — name owner
`agentstow`, and there are no long-lived tokens anywhere in the pipeline. The
moment the repo leaves that org every binding breaks, and npm's provenance check
compounds it: `package.json`'s `repository` must match the actual repo or the
publish is rejected outright. So a final `agentstow` 2.0.6 pointing users at
agent-sync has to be cut first, while the old bindings still work. Doing it
afterwards would mean registering trust entries twice — once to say goodbye,
once to say hello.

The vacated `agentstow` GitHub org is kept, empty. A renamed-away org name
becomes claimable by anyone, and 1884 npm downloads a month is enough traffic
that letting a stranger inherit the name is a real risk rather than a
theoretical one. The old crate and packages are deprecated and yanked rather
than deleted; crates.io could not delete them regardless.

Accepted cost: transferring the repo severs Cloudflare Workers Builds, whose
GitHub App is org-scoped and dashboard-only with no scriptable path — the same
finding recorded on 2026-08-13. The site stops auto-deploying until it is
reconnected by hand.

## 2026-09-16 — The rename needs no user migration, except the render marker

`~/.agents` survives untouched. The Commons was deliberately built with no
marker, manifest, provenance file or lockfile of agentstow's own (ADR-0004), and
`AGENTS.md`, `mcp.json`, the family directories, every symlink and the
`@~/.agents/AGENTS.md` import line are all name-neutral. Per-host migration is
therefore moving one config file and re-running sync, which is not worth
dual-read machinery for a project one month old with two stars. Doctor names the
leftover directory instead, the same warn-and-let-them-delete pattern the v1
`~/.agentstow` case already uses — now a third legacy layer.

The exception is `src/render.rs`'s `MARKER`, which is stamped as the first line
of every rendered command file on disk and matched by exact equality. Changing
it without accepting the old string would not error; every already-stamped file
would silently reclassify as Foreign, sync would write a duplicate beside it,
and the orphan sweep would skip it forever. `is_ours()` therefore has to accept
both markers, following the `PRE_V2_AGENTS_NAME` precedent.

## 2026-09-16 — The agent-sync release line is tagged `agent-sync-v*`

`1.0.0` was chosen as the first agent-sync version before anyone checked the tag
namespace it would land in. The repo already carries `v1.0.0` through `v2.0.5`,
transferring an org takes every tag with it, and `v1.0.0` on this remote is
`c304ef9` — a real, published GitHub Release whose asset URLs the Homebrew
formula still points at for anyone pinning an old version. Deleting the old tags
to free the name would break those downloads to solve a naming collision, so the
tags stay and the new line moves aside: `agent-sync-v1.0.0`, with the package
version still a plain `1.0.0`.

The prefix is not decoration; it is the only thing distinguishing two release
series that share one history. A reader landing on this repo sees `v2.0.5` and
`agent-sync-v1.0.0` and can tell which project each belongs to, which a bare
`1.0.0` sitting between `v1.0.0` and `v2.0.5` would actively obscure.

The cost is four places that parse the tag: the workflow's `tags: ["v*"]` filter
(`agent-sync-v*` is not matched by it — `v*` anchors at the start), the guard job
comparing tag to `Cargo.toml`, `update-formula.sh`'s `version="${tag#v}"`, and
the `pypi` GitHub environment's tag policy. The last is the dangerous one: an
environment policy that does not match does not fail the job with an error, it
refuses the deployment, so a wrong pattern there looks like a hang rather than a
bug. All four are changed together in step 4, before any 1.0.0 tag exists.

## 2026-09-16 — Marker compatibility is one-way; the old binary is retired

`src/render.rs` gets the new marker and `is_ours()` accepts both, so agent-sync
adopts files agentstow stamped. The reverse is deliberately not built: agentstow
2.0.6 ships without knowledge of the new marker, so running it after agent-sync
has rendered would see those files as Foreign and duplicate them.

Teaching 2.0.6 the future marker was possible — it is a string constant in a
release not yet tagged — and was rejected because it buys safety only for a user
who installs the successor and then goes back to the predecessor, keeps both on
`PATH`, and runs the old one again. The population that could do this is two
stars' worth of installs, the blast radius is `~/.gemini/commands/*.toml` alone
(the only surface `Commands::Render` writes), and the failure is a visible
duplicate file rather than data loss. Migration is: install agent-sync, delete
agentstow. Supporting an indefinite both-installed state is machinery for a
state nobody should be in.

This is the one place the rename is not symmetric, and it is why the deprecation
notices say the old package is replaced rather than superseded.

## 2026-09-16 — The npm deprecation points at the site, not at the package

All seven npm packages are deprecated, every version, with *"agentstow has been
renamed to agent-sync. See https://agent-sync.sh for the new package."*

The obvious wording was "install agent-sync-sh instead", and it is wrong today:
`agent-sync-sh` does not exist on npm until the bootstrap publish, because
trusted publishing cannot create a package. Shipping it now would repeat the
defect this release already had to fix once — the wheel and npm launcher both
told users to `cargo install agentstow`, a command that a later yank would
break. A deprecation notice is read at install time by someone who has just been
told to go elsewhere, so "elsewhere" has to resolve. The site does, and it says
plainly that the rename is mid-flight. **These messages must be re-set to name
the package once 1.0.0 is published**; `npm deprecate` overwrites freely, so
this costs one command and no version.

`cargo yank` is deliberately not run alongside this. Deprecation warns and still
installs, so 2.0.6 keeps working for the people the notice is talking to;
yanking the old line now would strand exactly that audience. It waits until the
successor is proven.

One casualty worth naming: deprecating without a version range rewrites every
version's message, so 1.1.2's specific note — *"binary shipped without the
executable bit; use 1.1.3"* — is gone. Accepted, because the whole line is now
superseded and 1.1.3 is no more alive than 1.1.2.

The account carries 2FA on **writes**, not just authorization, so every publish
and deprecate is an interactive operation. npm's browser approval flow needs one
approval per package; passing a single `--otp` to all seven back to back does it
in one. Worth knowing for the bootstrap publishes, and worth knowing that npm is
retiring the token-based bypass — the registry now warns that 2FA-bypassing
tokens end for account changes in Aug 2026 and for direct publishing in Jan 2027.
