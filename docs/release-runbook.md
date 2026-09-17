# Release runbook

Since 1.1.2 a release is published by CI: pushing an `agent-sync-vX.Y.Z` tag makes the
`release` workflow build, verify, and — only on the tag push — publish to
crates.io and npm. Both registries are authenticated with **OIDC trusted
publishing**: the workflow mints short-lived credentials per run, so there is
no long-lived token to store, rotate, or leak, and npm's 2FA enforcement is
satisfied without an OTP. `scripts/verify-packaging.sh` runs the same
verification locally.

## One-time setup — trusted publishing on all three registries

Done once per registry from the owning account (`soulmachine`); a publish from
CI fails with an auth error until this exists. The move to
`agent-sync-sh/agent-sync` invalidated every entry registered under the old
owner and repo, so all of them have to be re-registered against the names below.

- **crates.io** — <https://crates.io/crates/agent-sync-sh/settings> → *Trusted
  Publishing* → *Add*: repository owner `agent-sync-sh`, repository name
  `agent-sync`, workflow filename `release.yml`, environment left blank.
- **PyPI** — once the project exists the entry lives at
  <https://pypi.org/manage/project/agent-sync-sh/settings/publishing/>: owner
  `agent-sync-sh`, repository name `agent-sync`, workflow name `release.yml`,
  environment **`pypi`**. Before the first release, register it instead as a
  *pending* publisher from <https://pypi.org/manage/account/publishing/>: PyPI
  creates the project on the first successful OIDC upload, so unlike npm and
  crates.io it needs no bootstrap publish and no token ever exists.

  Unlike the other two registries this one names an environment, because PyPI
  recommends it. Three things must agree or the publish fails at token
  exchange: the `environment: pypi` key on the `publish-pypi` job, a GitHub
  environment named `pypi`, and this field. The environment additionally
  carries an `agent-sync-v*` **tag** deployment policy, so a run on a branch
  cannot reach it even though it sits in the same workflow file.

  The environment itself **survived the repo transfer** — only its tag policy
  was stale, still reading `v*` from the agentstow line. Read it with
  `gh api repos/agent-sync-sh/agent-sync/environments/pypi/deployment-branch-policies`;
  a policy is added with `-X POST -f name='agent-sync-v*' -f type=tag` and a
  stale one deleted by its id. Add before deleting, so the environment is never
  left without a policy.

  **This is the quiet one.** A wrong tag policy does not fail loudly — the job
  simply never receives a credential. Re-check the environment after any change
  to the tag namespace, which the rename was.
- **npm** — for **each of the seven packages** (`agent-sync-sh`,
  `@agent-sync-sh/darwin-arm64`, `@agent-sync-sh/darwin-x64`,
  `@agent-sync-sh/linux-arm64`, `@agent-sync-sh/linux-x64`,
  `@agent-sync-sh/win32-arm64`, `@agent-sync-sh/win32-x64`): package page →
  *Settings* → *Trusted Publisher* → *GitHub Actions*: organization
  `agent-sync-sh`, repository `agent-sync`, workflow filename `release.yml`,
  environment left blank. Trusted publishing also generates provenance
  attestations; the `repository` field every package carries must keep matching
  the GitHub repo exactly or the publish is rejected.

## Bootstrapping the names

**Trusted publishing authenticates a publish; it does not create a package.**
Neither crates.io nor npm will create a name from an OIDC token, so each needs
one manual, credentialed publish before its trust entry can be registered at
all. PyPI is the exception.

| Registry | Name | Created by OIDC? |
|---|---|---|
| crates.io | `agent-sync-sh` | no — `cargo publish` once with a token |
| npm | `agent-sync-sh`, `@agent-sync-sh/<target>` ×6 | no — `npm publish` once with an OTP |
| PyPI | `agent-sync-sh` | yes — register a *pending* publisher instead |

### The `agent-sync-sh` npm organisation

The six platform packages are scoped `@agent-sync-sh/<target>`, and a scoped
name needs the scope to exist and the publishing account to belong to it:

1. Sign in to npmjs.com as `soulmachine`.
2. Create an organisation named exactly `agent-sync-sh`
   (<https://www.npmjs.com/org/create>). The **free** tier is enough — public
   packages only, which is what these are.
3. `npm org ls agent-sync-sh` should list `soulmachine` as an owner. With a
   credential from `npm login` this is a reliable probe, and the distinction is
   worth knowing: a scope that exists prints its members, and one that does not
   fails `E404 Scope not found` — the same error a nonsense scope gives. (An
   older note here said a 403 made it useless. That was the dead publish-only
   token, not the login session.)

**A passing dry run proves nothing here.** `npm publish --dry-run` packs and
validates locally; it does not check that the scope exists or that you may
publish into it. Under the old name four platform packages reported `ok` while
the scope was still unclaimed, which is why this section exists rather than
trusting the script.

If a name turns out to be taken, the alternative is unscoped
(`agent-sync-sh-darwin-arm64` and so on). That changes `optionalDependencies`
in `npm/agent-sync-sh/package.json` and the naming in `scripts/build-npm.sh`,
and nothing else — the launcher resolves whatever those names say.

### npm 2FA on the bootstrap publishes

The account enforces 2FA on **writes**, not only on sign-in, so each bootstrap
publish needs an OTP or it fails with `EOTP`. The browser approval flow asks
once **per package** and the approval expires within minutes, so seven
interactive approvals in a row is not workable: read one fresh code and pass it
as `--otp` to all seven back-to-back.

npm is retiring 2FA-bypass tokens — account changes from August 2026, direct
publishing from January 2027 — so this bootstrap path has a shelf life. It is
one more reason not to add platform packages casually.

## Releasing

1. Bump `version` in `Cargo.toml`. Nothing else carries a version. The crate
   requires Rust 1.97 (edition 2024), so a release machine needs a current
   toolchain.
2. `./scripts/verify-packaging.sh` — must end with *Local packaging checks
   passed* and no blocked packages.
3. Commit, tag `agent-sync-vX.Y.Z`, push the tag. The tag must match `Cargo.toml` — a
   `guard` job fails the run otherwise. The `release` workflow cross-builds
   all six targets, assembles the packages, installs them offline, dry-run
   publishes, then publishes the crate, all seven npm packages and the six
   PyPI wheels, attaches the six binaries to the GitHub Release, and commits
   the regenerated Homebrew formula to main. The registry publish jobs run only on the tag
   push — never for `workflow_dispatch` or pull requests. The `release` and
   `tap` jobs are gated on the ref instead, so both also run for a
   `workflow_dispatch` made **at an `agent-sync-v*` tag**: the binaries and the formula can
   be rebuilt without moving the tag, and re-runs overwrite the assets rather
   than failing.
4. Verify as described below once the workflow is green.

### npm's trusted publishing has never actually run

**Watch the npm job on the next release — it has published nothing so far.**
The 1.0.0 workflow run was green, but its npm job did no work: the bootstrap
publishes had already put 1.0.0 on the registry with real credentials, so all
seven packages hit the idempotence check and reported `already on the registry;
skipping` (run `35214086412`, job *publish to npm*). The seven
trusted-publisher entries on npmjs.com have therefore never authenticated a
publish, and the first one to exercise them is the next version bump.

crates.io is exercised only as far as minting the OIDC token, which happens in
a step *before* its own idempotence skip. PyPI is the one channel proven end to
end — it created the project from the pending publisher on that run.

So a green npm job on the 1.0.0 run is not evidence the path works. If the next
release fails there, the fix is registry-side and needs no tag move: see the
re-run note below.

## Recovering a half-published release

A run can publish some channels and fail before others: 2.0.6 reached
crates.io, npm, the GitHub Release and the formula, then failed the wheels job
and skipped PyPI. The publish jobs fire **only on a tag push**, so a
`workflow_dispatch` cannot finish the job — recovery is either moving the tag
onto a fixed commit or bumping the version. Moving it is usually right: every
publish job is idempotent (crates.io checks the index, npm runs `npm view` per
package, PyPI passes `skip-existing`), so the channels already done skip
themselves and only the missing one publishes.

**A registry-side failure needs no tag move at all.** When the fix lives outside
the repository — a trusted-publisher entry, an environment's tag policy, a
credential — `gh run rerun <run-id> --failed` replays just the failed jobs under
the *original* event, so `github.event_name == 'push'` still holds and the
publish jobs fire. Only a fix that changes the commit needs the tag moved, and
moving it is what springs the draft-Release trap below. Reach for the re-run
first.

Moving a tag has one trap that does not announce itself. **Deleting a tag
demotes its GitHub Release to a draft**, and a draft's assets return 404 to
everyone — which breaks `brew install`, because the formula points at those
asset URLs. The re-run does not repair it: `gh release view` finds the draft,
clobbers the assets into it, and reports success, so the workflow goes green
while the tap is broken. After any tag move, publish it again and prove an
anonymous download works:

```sh
gh release edit agent-sync-vX.Y.Z --draft=false --verify-tag
curl -fsSLI https://github.com/agent-sync-sh/agent-sync/releases/download/agent-sync-vX.Y.Z/agent-sync-X.Y.Z-darwin-arm64.tar.gz
```

GitHub's asset CDN trails the un-draft by around half a minute, so a 404 in the
first few seconds is not yet a problem; retry before concluding anything.

The builds are **not** byte-reproducible — a re-run of the same commit produced
four different tarball checksums — so the formula is regenerated and recommitted
on every replay. That is self-consistent, because the assets are clobbered in
the same run, but a formula left over from an earlier run is stale. Verify the
formula against the live assets rather than assuming, as *Verifying* describes.

## Manual fallback

If CI publishing is unavailable, publish by hand from the workflow's
`npm-packages` artifact (or a local `scripts/build-npm.sh dist`):

1. `cargo publish`.
2. Publish the **platform packages first**, then the launcher:
   ```sh
   for target in darwin-arm64 darwin-x64 linux-arm64 linux-x64; do
     (cd "dist/$target" && npm publish --access public)
   done
   (cd dist/agent-sync-sh && npm publish --access public)
   ```
   Order matters. The launcher declares the platform packages as optional
   dependencies; publishing it first leaves a window where installing it
   resolves nothing and the binary is missing.
   `--access public` is required: scoped packages default to restricted.
   CI publishes in this same order.

## The PyPI wheels

`scripts/build-wheels.py` packages the **already-built** binaries into one wheel
per platform. Nothing is compiled: there is no `pyproject.toml`, no maturin, and
no Python code in the wheels — each carries the same binary the tarball and the
npm package ship, in `agent_sync_sh-<version>.data/scripts/`, which pip installs
straight onto PATH.

A seventh wheel, `py3-none-any`, carries no binary at all — just a console
script that names the platform and points at `cargo install`. pip always ranks a
platform tag above `any`, so it is reached only where nothing else matches;
without it, an unsupported platform gets pip's bare *no matching distribution
found*, which names neither the cause nor a way forward. This mirrors the npm
launcher, which also installs cleanly and explains itself when run.

Three things are easy to get wrong and are guarded in CI:

- **The platform tag.** It is written by hand per target; get it wrong and pip
  reports *no matching distribution* rather than anything pointing at the cause.
  The `wheels` job installs the manylinux wheel on the runner to prove at least
  one tag resolves.
- **The executable bit.** pip decides with `stat.S_ISREG(mode) and mode & 0o111`,
  so the zip entry needs `S_IFREG` set, not a bare `0o755`. Without it pip
  installs a **non-executable** `agent-sync` to the venv's `bin/` — a command on
  PATH that cannot run, and one that `--version` in the build never catches
  because the build never installs. The `wheels` job asserts `test -x` after a
  real `pip install` for exactly this reason.
- **Which wheel a check installs.** `py3-none-any` sorts first, so a naive
  `ls | head -1` tests the fallback and reports the binary as broken. Both CI
  and `verify-packaging.sh` name the platform wheel explicitly, assert pip
  prefers it when both are offered, and assert the fallback exits non-zero
  with a message.

**Manual fallback.** With the binaries staged under `target/<triple>/release/`:

```sh
scripts/build-wheels.py wheelhouse
python -m twine upload wheelhouse/*.whl
```

A locally built wheel only ever contains the host's own binary — the script
refuses to seal a host binary into another platform's wheel, since a wheel on
PyPI can be yanked but never replaced. For the same reason it runs the host
binary's `--version` and refuses to package when it disagrees with
`Cargo.toml`, which is what a stale `target/release/` from before a version
bump would otherwise produce.

## The Homebrew tap

The tap is this repository. There is no separate `homebrew-agent-sync` repo, so
users tap it by URL — the short `brew tap agent-sync-sh/agent-sync` form would look
for `agent-sync/homebrew-agent-sync` and 404:

```sh
brew tap agent-sync-sh/tap https://github.com/agent-sync-sh/agent-sync
brew trust agent-sync-sh/tap      # Homebrew 6 will not load an untrusted third-party tap
brew install agent-sync
```

`Formula/agent-sync.rb` is **generated — never hand-edit it.** The `tap` job runs
`scripts/update-formula.sh <tag>`, which reads the `SHA256SUMS.txt` already
published on that release and rewrites the file whole, then commits it to main.
Two consequences worth knowing:

- The formula can only ever describe assets that exist; the script exits
  non-zero rather than emitting a formula with a missing or malformed sha256.
- It regenerates rather than patches, so there is no half-updated state where
  the version moved and a sha256 did not.

It carries no `version` stanza on purpose — Homebrew scans the version out of
the asset URL, and `brew audit` rejects the redundant stanza.

**Manual fallback.** If the `tap` job fails but the release assets are up:

```sh
scripts/update-formula.sh agent-sync-vX.Y.Z
git add Formula/agent-sync.rb && git commit -m "Homebrew formula: agent-sync-vX.Y.Z" && git push
```

**Verifying the tap** (`brew fetch` proves the URL and checksum without
installing anything):

```sh
brew tap agent-sync-sh/tap https://github.com/agent-sync-sh/agent-sync
brew trust agent-sync-sh/tap
brew info agent-sync          # should report the version just released
brew audit agent-sync-sh/tap/agent-sync
brew fetch agent-sync
```

## Verifying

1. **Wait for propagation before verifying.** A package name that is new to the
   registry is not readable the instant `npm publish` returns, even though the
   upload succeeded. On the 1.0.0 release all four `@agent-sync-sh/*` packages
   returned `PUT 200` and then 404 on `GET` for several minutes, appearing one
   at a time; the `agent-sync-sh` launcher was visible immediately only because that
   name already existed. A 404 straight after publishing is not a failed
   publish — check the npm debug log for `PUT 200` before assuming anything is
   wrong, and re-check the registry rather than republishing.
   ```sh
   until curl -sf -o /dev/null https://registry.npmjs.org/@agent-sync-sh%2Fdarwin-arm64; do sleep 15; done
   ```
2. Verify from a clean directory, with the cache cleared so a stale packument
   cannot mask the result:
   ```sh
   npm cache clean --force
   npm install --no-save agent-sync-sh && ./node_modules/.bin/agent-sync --version
   ```

## Retiring the agentstow line

The npm packages are **deprecated**, which warns and still installs, and their
notice now names `agent-sync-sh`, so a reader sent elsewhere has somewhere that
resolves. That destination existing is what made the crate yank safe to bring
forward.

`cargo yank` was **done on 2026-09-17**, ahead of the schedule this section
originally set, by the owner's explicit decision rather than by the condition
below. **All thirteen published versions are yanked** — 1.0.0, 1.1.0-1.1.3,
1.2.0 and 2.0.0-2.0.6 — in two passes, 2.0.x first and 1.x once the owner
confirmed the whole line should go:

    cargo yank --version <v> agentstow     # for each of the thirteen

Confirmed against the index: every line of
<https://index.crates.io/ag/en/agentstow> reads `"yanked":true`. Nothing under
`agent-sync-sh` was touched. To undo any of it: `cargo yank --undo --version <v>
agentstow`.

Yanking blocks new dependency resolution but leaves existing `Cargo.lock` files
working, so it is the mild end of retirement. `Formula/agentstow.rb` is already
frozen and carries Homebrew's own `deprecate!`; the npm packages are already
deprecated. Nothing else about the old line needs action.

`agent-sync.sh` was **submitted to the HSTS preload list on 2026-09-17** and is
`pending`. Getting there needed two Cloudflare zone changes, neither of which
lives in this repo: `always_use_https` was `off`, so plain HTTP served 200
instead of redirecting, and the zone's HSTS header was disabled entirely. Both
are now set (`max-age=31536000; includeSubDomains; preload`). Check progress at
<https://hstspreload.org/api/v2/status?domain=agent-sync.sh>. **Removal takes
months**, so every future subdomain of `agent-sync.sh` must be served over
HTTPS — today only the apex and `www` exist.

**All four Cloudflare zones** — `agent-sync.sh` and the three `agentstow.*`
redirect zones — had `min_tls_version` raised from `1.0` to **`1.2`** on
2026-09-17. Cloudflare's default floor is simply old; this is unrelated to
preload. Verified by handshake on every zone: TLS 1.0 and 1.1 are refused, 1.2
and 1.3 are served, a default client negotiates 1.3 over HTTP/2, and each
`agentstow.*` apex plus `www` still 301s to `https://agent-sync.sh/` and
follows through to a 200.

The redirect zones were raised **by the owner's explicit decision**, overriding
the recommendation to leave them: the argument for holding them at `1.0` was
that a client too old for TLS 1.2 is the one that most needs the 301 to work.
That is the known cost — such a client now cannot reach the redirect at all. It
is a deliberate trade for a uniform floor, not an oversight, so do not "fix" it
back. Reverting is a one-line `PATCH` of `min_tls_version` per zone.

## Notes

- **2FA.** The account enforces 2FA for publishing. CI is untouched by this:
  trusted publishing mints per-run credentials that satisfy the enforcement
  without an OTP. The **manual fallback** still prompts — `--otp=<code>` skips
  the browser round trip. Granular tokens that bypass 2FA are being restricted
  from January 2027, which is exactly why CI uses trusted publishing and not a
  stored token.
- **Artifacts strip permissions.** `actions/download-artifact` does not
  preserve file modes, so any job consuming the `npm-packages` artifact must
  re-`chmod +x` the binaries before publishing — the publish job does, and
  proves it by executing the linux-x64 binary. 1.1.2 shipped a non-executable
  binary because this step was missing.
- **No install hooks, ever.** The packages carry no `preinstall`, `install` or
  `postinstall` script. That is what makes an install work offline and inside a
  sandboxed CI, and it is asserted by both the local script and the workflow.
  Anything that would need a postinstall fetch belongs in a platform package
  instead.
- **Unsupported platforms.** A machine with no matching platform package gets a
  message naming the package it looked for and pointing at `cargo install`,
  rather than a missing-file crash. Since 1.2.0 the built targets are macOS,
  Linux and Windows, x64 and arm64 each; win32-arm64 is cross-compiled and is
  the one target CI never executes.
- **A new platform package cannot bootstrap itself.** npm trusted publishing
  only publishes into packages that already exist, so the *first* release of a
  new `@agent-sync-sh/*` package must be published manually with an OTP (from a
  local `scripts/build-npm.sh dist` or the CI artifact), after which its
  trusted publisher is configured on npmjs.com and CI takes over.
