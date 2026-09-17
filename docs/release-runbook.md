# Release runbook

Since 1.1.2 a release is published by CI: pushing a `vX.Y.Z` tag makes the
`release` workflow build, verify, and — only on the tag push — publish to
crates.io and npm. Both registries are authenticated with **OIDC trusted
publishing**: the workflow mints short-lived credentials per run, so there is
no long-lived token to store, rotate, or leak, and npm's 2FA enforcement is
satisfied without an OTP. `scripts/verify-packaging.sh` runs the same
verification locally.

## One-time setup — trusted publishing on both registries

Done once per registry from the owning account (`soulmachine`); a publish from
CI fails with an auth error until this exists.

- **crates.io** — <https://crates.io/crates/agentstow/settings> → *Trusted
  Publishing* → *Add*: repository owner `agentstow`, repository name
  `agentstow`, workflow filename `release.yml`, environment left blank.
- **PyPI** — now that the project exists, the entry lives at
  <https://pypi.org/manage/project/agentstow/settings/publishing/>: owner
  `agentstow`, repository name `agentstow`, workflow name `release.yml`,
  environment **`pypi`**. Before the first release it was instead registered as
  a *pending* publisher from <https://pypi.org/manage/account/publishing/>;
  PyPI creates the project on the first successful OIDC upload, so unlike npm
  and crates.io there was no manual bootstrap publish and no token ever
  existed.

  Unlike the other two registries this one names an environment, because PyPI
  recommends it and this entry was the newest. Three things must agree or the
  publish fails at token exchange: the `environment: pypi` key on the
  `publish-pypi` job, a GitHub environment named `pypi`, and this field. The
  environment additionally carries a `v*` **tag** deployment policy, so a run
  on a branch cannot reach it even though it sits in the same workflow file.
- **npm** — for **each package** (`agentstow`, `@agentstow/darwin-arm64`,
  `@agentstow/darwin-x64`, `@agentstow/linux-arm64`, `@agentstow/linux-x64`,
  `@agentstow/win32-arm64`, `@agentstow/win32-x64`): package page → *Settings*
  → *Trusted Publisher* → *GitHub Actions*: organization `agentstow`,
  repository `agentstow`, workflow filename `release.yml`, environment left
  blank. Trusted publishing also generates provenance attestations; the
  `repository` field every package already carries must keep matching the
  GitHub repo or the publish is rejected.

## Prerequisites — both settled 2026-08-13

Frank claimed the organisation and set the version to `1.0.0`. Both checks below
now pass; they are kept because they are what to re-run if a release ever fails
at the publish step.

### 1. The `@agentstow` npm organisation — **claimed**

The four platform packages are named `@agentstow/darwin-arm64`,
`@agentstow/darwin-x64`, `@agentstow/linux-arm64`, `@agentstow/linux-x64`. A
scoped name requires the scope to exist and the publishing account to belong to
it. Verified on 2026-08-13:

```
$ npm view @agentstow/darwin-arm64 version
npm error 404 Not Found

$ npm org ls agentstow
npm error 403 Forbidden — You may not perform that action with these credentials.
```

Those were the readings *before* the org existed. Note that `npm org ls` still
returns 403 from this machine's credentials: the CLI token can publish but is
not authorised to read organisation membership, so a 403 there is not evidence
either way. The publish itself is what settles it. To claim the scope:

1. Sign in to npmjs.com as `soulmachine` (the account that owns `agentstow`).
2. Create an organisation named exactly `agentstow`
   (<https://www.npmjs.com/org/create>). The **free** tier is enough — public
   packages only, which is what these are.
3. Confirm it worked: `npm org ls agentstow` should list `soulmachine` as an
   owner rather than returning 403.

**A passing dry run never proved this.** `npm publish --dry-run` packs and
validates locally; it does not check that the scope exists or that you may
publish into it. The four platform packages reported `ok` while the scope was
still unclaimed, so that signal is worth nothing here — which is why this
section exists rather than trusting the script.

If the name turns out to be taken, the alternative is unscoped names
(`agentstow-darwin-arm64` and so on). That changes `optionalDependencies` in
`npm/agentstow/package.json` and the naming in `scripts/build-npm.sh`, and
nothing else — the launcher resolves whatever those names say.

### 2. The published version — **set to `1.0.0`**

`agentstow@0.0.1` is the name-claim placeholder, published 2026-08-13 and owned
by `soulmachine`. A release cannot reuse it:

```
$ npm publish --dry-run       # in npm/agentstow
npm error You cannot publish over the previously published versions: 0.0.1.
```

`Cargo.toml` now says `1.0.0`, and `scripts/verify-packaging.sh` confirms all
five packages dry-run cleanly at that version. `scripts/build-npm.sh` reads the
version from `Cargo.toml` and stamps every package, so the crate and all five
npm packages stay in lockstep by construction — a release is a one-line bump.

## Releasing

1. Bump `version` in `Cargo.toml`. Nothing else carries a version. The crate
   requires Rust 1.97 (edition 2024), so a release machine needs a current
   toolchain.
2. `./scripts/verify-packaging.sh` — must end with *Local packaging checks
   passed* and no blocked packages.
3. Commit, tag `vX.Y.Z`, push the tag. The tag must match `Cargo.toml` — a
   `guard` job fails the run otherwise. The `release` workflow cross-builds
   all six targets, assembles the packages, installs them offline, dry-run
   publishes, then publishes the crate, all seven npm packages and the six
   PyPI wheels, attaches the six binaries to the GitHub Release, and commits
   the regenerated Homebrew formula to main. The registry publish jobs run only on the tag
   push — never for `workflow_dispatch` or pull requests. The `release` and
   `tap` jobs are gated on the ref instead, so both also run for a
   `workflow_dispatch` made **at a v\* tag**: the binaries and the formula can
   be rebuilt without moving the tag, and re-runs overwrite the assets rather
   than failing.
4. Verify as described below once the workflow is green.

## Recovering a half-published release

A run can publish some channels and fail before others: 2.0.6 reached
crates.io, npm, the GitHub Release and the formula, then failed the wheels job
and skipped PyPI. The publish jobs fire **only on a tag push**, so a
`workflow_dispatch` cannot finish the job — recovery is either moving the tag
onto a fixed commit or bumping the version. Moving it is usually right: every
publish job is idempotent (crates.io checks the index, npm runs `npm view` per
package, PyPI passes `skip-existing`), so the channels already done skip
themselves and only the missing one publishes.

Moving a tag has one trap that does not announce itself. **Deleting a tag
demotes its GitHub Release to a draft**, and a draft's assets return 404 to
everyone — which breaks `brew install`, because the formula points at those
asset URLs. The re-run does not repair it: `gh release view` finds the draft,
clobbers the assets into it, and reports success, so the workflow goes green
while the tap is broken. After any tag move, publish it again and prove an
anonymous download works:

```sh
gh release edit vX.Y.Z --draft=false --verify-tag
curl -fsSLI https://github.com/agentstow/agentstow/releases/download/vX.Y.Z/agentstow-X.Y.Z-darwin-arm64.tar.gz
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
   (cd dist/agentstow && npm publish --access public)
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
npm package ship, in `agentstow-<version>.data/scripts/`, which pip installs
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
  installs a **non-executable** `agentstow` to the venv's `bin/` — a command on
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

The tap is this repository. There is no separate `homebrew-agentstow` repo, so
users tap it by URL — the short `brew tap agentstow/agentstow` form would look
for `agentstow/homebrew-agentstow` and 404:

```sh
brew tap agentstow/tap https://github.com/agentstow/agentstow
brew trust agentstow/tap      # Homebrew 6 will not load an untrusted third-party tap
brew install agentstow
```

`Formula/agentstow.rb` is **generated — never hand-edit it.** The `tap` job runs
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
scripts/update-formula.sh vX.Y.Z
git add Formula/agentstow.rb && git commit -m "Homebrew formula: vX.Y.Z" && git push
```

**Verifying the tap** (`brew fetch` proves the URL and checksum without
installing anything):

```sh
brew tap agentstow/tap https://github.com/agentstow/agentstow
brew trust agentstow/tap
brew info agentstow          # should report the version just released
brew audit agentstow/tap/agentstow
brew fetch agentstow
```

## Verifying

1. **Wait for propagation before verifying.** A package name that is new to the
   registry is not readable the instant `npm publish` returns, even though the
   upload succeeded. On the 1.0.0 release all four `@agentstow/*` packages
   returned `PUT 200` and then 404 on `GET` for several minutes, appearing one
   at a time; the `agentstow` launcher was visible immediately only because that
   name already existed. A 404 straight after publishing is not a failed
   publish — check the npm debug log for `PUT 200` before assuming anything is
   wrong, and re-check the registry rather than republishing.
   ```sh
   until curl -sf -o /dev/null https://registry.npmjs.org/@agentstow%2Fdarwin-arm64; do sleep 15; done
   ```
2. Verify from a clean directory, with the cache cleared so a stale packument
   cannot mask the result:
   ```sh
   npm cache clean --force
   npm install --no-save agentstow && ./node_modules/.bin/agentstow --version
   ```

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
  new `@agentstow/*` package must be published manually with an OTP (from a
  local `scripts/build-npm.sh dist` or the CI artifact), after which its
  trusted publisher is configured on npmjs.com and CI takes over.
