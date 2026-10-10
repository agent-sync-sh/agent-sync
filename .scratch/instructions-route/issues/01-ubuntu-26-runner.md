# 01 — Check CI and the release workflow on Ubuntu 26

**What to build:** GitHub moves the `ubuntu-latest` runner label to Ubuntu 26 from 2026-10-19 (actions/runner-images#14748; every `ci` run since 1.2.0 carries the notice). In `ci.yml`, the `fmt`, `msrv` and `linux` jobs run on it. In `release.yml`, ten jobs run on it, including `publish-crate`, `publish-npm` and `publish-pypi`, so the next release is their first run on the new image. Read the first `ci` run on main after 2026-10-19. If a job fails because of the image, pin it to `ubuntu-24.04` with a one-line comment that says why. Before the next release, decide whether to pin the `release.yml` jobs too, because a failure there can leave a half-published release (see the runbook's "Recovering a half-published release").

**Status:** ready-for-human

- [ ] The first `ci` run on main after 2026-10-19 is read
- [ ] Green: no change. Red because of the image: the failing jobs are pinned to `ubuntu-24.04`, with the reason in a comment
- [ ] The `release.yml` jobs are pinned or knowingly left on `ubuntu-latest` before the next tag
