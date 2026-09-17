# Deploying agent-sync.sh

## Current state

Deployed as the Worker `agent-sync-site` (Workers Static Assets). The apex domain and its
certificate are attached by the `custom_domain` route in `wrangler.toml`; no DNS record is
created by hand.

- [x] First deploy — live at <https://agent-sync.sh> (2026-09-17)
- [x] `www.agent-sync.sh` → 301 to the apex
- [x] `agentstow.dev`, `agentstow.com`, `agentstow.org` (and their `www`) → 301 to the apex
- [ ] Auto-deploy on push to `main` — Workers Builds must be reconnected (see below)

Until 2026-09-17 the site lived at `agentstow.dev` as the Worker `agentstow-site`. That
Worker is deleted; `agentstow.dev` is now a redirect-only zone like `.com` and `.org`.

## Deploying

```sh
cd site
npx wrangler deploy
```

Auth comes from `CLOUDFLARE_API_TOKEN` in `~/.zshenv`; there is no wrangler OAuth config on
this machine. Zone ids: `agent-sync.sh` is `756b0419e2567503efca62502cbb7938`;
`agentstow.dev` is `de5a3b0f14b051def606f3779a439641`.

## Redirects

`_redirects` cannot do domain-level redirects, so each redirecting hostname needs two things:
a proxied DNS record so Cloudflare has something to terminate, and a zone Redirect Rule (a
`http_request_dynamic_redirect` ruleset) sending it to `https://agent-sync.sh` with a 301.

An `AAAA` record pointing at `100::` (the discard prefix), proxied, is the conventional
target for a redirect-only hostname — that is what Cloudflare's own custom-domain
attachment produces.

The API token on this machine has DNS and Ruleset write scope, so all of this is scripted.
What exists:

| Zone | DNS | Redirect rule |
| :-- | :-- | :-- |
| `agent-sync.sh` | `www` AAAA `100::` proxied (apex is the Worker's own record) | `www.agent-sync.sh` |
| `agentstow.dev` | apex + `www`, AAAA `100::` proxied | `agentstow.dev`, `www.agentstow.dev` |
| `agentstow.com` | apex + `www`, AAAA `100::` proxied | `agentstow.com`, `www.agentstow.com` |
| `agentstow.org` | apex + `www`, AAAA `100::` proxied | `agentstow.org`, `www.agentstow.org` |

Each rule is a single `http_request_dynamic_redirect` entry, 301, target
`concat("https://agent-sync.sh", http.request.uri.path)` with `preserve_query_string`. The
`cf` CLI has no zone-ruleset command, so the rules are written with the REST API:

```sh
curl -X PUT -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" -H "Content-Type: application/json" \
  "https://api.cloudflare.com/client/v4/zones/$ZONE/rulesets/phases/http_request_dynamic_redirect/entrypoint" \
  --data '{"rules":[{"action":"redirect","enabled":true,
    "expression":"(http.host eq \"HOST\" or http.host eq \"www.HOST\")",
    "action_parameters":{"from_value":{"status_code":301,"preserve_query_string":true,
      "target_url":{"expression":"concat(\"https://agent-sync.sh\", http.request.uri.path)"}}}}]}'
```

`PUT` on the entrypoint replaces the zone's whole redirect ruleset — fine here, each zone
has exactly one rule.

> **Expect 522s for the first minute.** A newly added hostname answers with 522 until its
> rule and certificate finish propagating — the request reaches the edge and is proxied to
> the `100::` discard address before the redirect rule is live. It resolves itself. Do not
> "fix" it.

## Auto-deploy

**Reconnect pending as of 2026-09-17.** The previous Worker was deployed by Workers Builds
from `agentstow/agentstow`; the repo transfer to `agent-sync-sh/agent-sync` and the new
Worker name orphaned that connection. Until it is redone, deploy by hand with
`npx wrangler deploy`.

Reconnect under **Workers & Pages → `agent-sync-site` → Settings → Build** — dashboard only;
there is no CLI or API path (see the history note below). Connect only after `main` carries
`name = "agent-sync-site"` in `wrangler.toml`; the initial build runs against whatever
`main` is, and a name mismatch fails it. The configuration to recreate:

| Setting | Value |
| :-- | :-- |
| Git repository | `agent-sync-sh/agent-sync` |
| Root directory | `site` |
| Build command | *(none)* |
| Deploy command | `npx wrangler deploy` |
| Version command | `npx wrangler versions upload` |
| Production branch | `main` |
| Builds for non-production branches | **on** |
| Build watch paths → include | `site/*` |

The Cloudflare GitHub App has to be installed on the `agent-sync-sh` org, scoped to this
one repository. Cloudflare authenticates builds with its own auto-minted API token (`Workers
Builds - <timestamp>`, visible under Settings → Build); no repository secret is involved. The
Worker name in the dashboard must stay `agent-sync-site` — it has to match `name` in
`wrangler.toml` or the build fails.

Once connected: a push to `main` that touches `site/` deploys the site, a push touching
nothing under `site/` is skipped before a build is queued, and non-production branches get
preview versions via `npx wrangler versions upload` without promoting them.

### History: this replaced a GitHub Actions deploy

Until 2026-08-15 the deploy lived in `.github/workflows/deploy-site.yml`, authenticated by a
scoped `CF_DEPLOY_TOKEN` repository secret — deliberately: the 2026-08-13 survey (docs,
`wrangler`, the `cf` CLI, and the REST API, each checked separately) found no scriptable way
to connect a repository to Workers Builds, and in-repo configuration was judged worth the
token upkeep. Those findings still hold; the connection was made by hand in the dashboard.
The decision was reversed to converge with `soulmachine/openroutine`, which deploys the same
way. The workflow was deleted, the repo secret removed, the Cloudflare-side token revoked,
and the workflow's post-deploy smoke checks moved into [Verifying](#verifying) below.

## Verifying

```sh
curl -s -o /dev/null -w '%{http_code}\n' https://agent-sync.sh        # 200
curl -s -o /dev/null -w '%{http_code}\n' https://agent-sync.sh/docs   # 200
curl -s -o /dev/null -w '%{http_code}\n' https://agent-sync.sh/zh     # 200
curl -s -o /dev/null -w '%{http_code}\n' https://agent-sync.sh/zh/docs # 200
curl -s -o /dev/null -w '%{http_code}\n' https://agent-sync.sh/nope   # 404, served not thrown
curl -s https://agent-sync.sh/zh/nope | grep -q 'lang="zh'            # the *Chinese* 404
curl -sI https://www.agent-sync.sh        | grep -i location
curl -sI https://agentstow.dev            | grep -i location
curl -sI https://agentstow.com            | grep -i location
curl -sI https://agent-sync.sh | grep -i content-security-policy      # script-src 'self'
```

Prefer GET (`-o /dev/null -w '%{http_code}'`) over HEAD (`-I`) for status checks — the edge
answers HEAD inconsistently on freshly deployed assets. Expect a minute of intermittent
errors right after a deploy while the new version propagates; sample a dozen requests before
concluding anything is wrong.
