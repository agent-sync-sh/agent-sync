# agent-sync.sh

The source for <https://agent-sync.sh>. It lives here, inside the repo it documents, so a
change to the CLI and the change it implies on the website can be one commit and one
review.

```
wrangler.toml     Cloudflare Workers Static Assets config
og.html           source for public/og.png (not published — outside public/)
public/           everything that gets served
public/zh.html    Simplified Chinese landing page, served at /zh
public/zh/        Chinese docs and 404, served at /zh/docs and for /zh/* misses
public/lang.js    the one script: language preference and first-visit detection
```

## Working on it

Hand-written HTML and one CSS file. There is no build step and no npm dependency. The
only JavaScript is `public/lang.js` — ~30 dependency-free lines that honor the
English/中文 switcher choice and, on a first visit to an English page, redirect
Simplified-Chinese browsers to the `/zh` twin. `public/_headers` declares
`script-src 'self'`, which admits exactly that file: no inline script, no third-party
script, and everything still works with JavaScript disabled.

```sh
npx wrangler dev        # from this directory; serves public/ locally
npx wrangler deploy     # ships it
```

`docs.html` is served at `/docs` — Workers Static Assets does the extensionless mapping,
so link to `/docs`, never `/docs.html`.

## Design

`public/styles.css` is structured after [vercel.com/design.md](https://vercel.com/design.md),
with three departures documented in a comment at the top of the file: a platform font
stack instead of Geist, a warm ramp read as "monochrome", and a palette defined here
rather than inherited, because agent-sync has no web UI.

The accent colour is spent on exactly four things — link text, the one primary button, the
focus ring, and the hairline on the mark. Code samples use weight and dimming rather than
colour, so they survive greyscale.

## Regenerating the artwork

`public/og.png` is a screenshot of `og.html`, taken with the light scheme forced — headless
Chrome inherits the Mac's appearance, so a plain `--screenshot` renders the dark theme on a
dark desktop and no Chrome flag overrides it. `playwright-cli` refuses `file://`, hence the
throwaway server:

```sh
python3 -m http.server 8765 --bind 127.0.0.1 &
playwright-cli run-code "async page => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1200, height: 630 });
  await page.goto('http://127.0.0.1:8765/og.html');
  await page.screenshot({ path: 'public/og.png' });
}"
playwright-cli close; kill %1; rm -rf .playwright-cli
```

The favicons come from an inline SVG of the mark, rasterised with ImageMagick at 32, 180
and 192 px. Both commands are only needed when the artwork or the palette changes.

## Accuracy

Every claim on these pages traces to `CONTEXT.md`, `docs/adr/`, or
`.scratch/agentstow-v1/spec.md` in the repo root. Two things in particular must not drift:

- **Memory sync is not a feature.** It is an explicit non-goal in the spec. The site says
  so out loud; do not let a tagline reintroduce it.
- The capability matrix on the landing page is the one in `spec.md`. If the registry in
  `src/registry.rs` changes, both move together.

**The Chinese mirror.** `public/zh.html`, `public/zh/docs.html` and `public/zh/404.html`
are hand-maintained Simplified Chinese twins of `index.html`, `docs.html` and `404.html`,
served at `/zh` and `/zh/docs`. They are translations, not a second site: any commit that
changes user-visible content on an English page must make the matching change to its zh
twin in the same commit — which means the capability matrix now moves in three places
with `src/registry.rs`: spec, en, zh. Code samples, CLI output, and mechanism/status
tokens (`fan-out`, `key-merge`, `linked`, `variant`, …) stay English on the zh pages;
only authored commentary — `<span class="c">` comment lines and illustrative tree
annotations inside `<pre>` — is translated. Section `id`s are identical across languages
so `#fragment` deep links work in both. Keep Chinese prose on single source lines: a
source line break inside a CJK sentence renders as a stray space in Chrome, so the zh
files do not hard-wrap paragraphs the way the English files do.
