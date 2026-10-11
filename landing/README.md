# Flow landing page

A static, dependency-free product page for Flow: `index.html`, `styles.css`, `main.js`, the app
favicon, and product stills and clips captured from the real UI.

- **Design**: Apple Human Interface Guidelines — system type, semantic light/dark colours,
  translucent bars, and motion that turns off under *Reduce Motion*.
- **Get Flow** links to the latest GitHub release when one exists (looked up at page load) and to
  the repository otherwise.
- **Footer**: the pinned credit strip used on Picky's landing page.

## Preview

Serve the folder with any static server, for example:

```sh
python3 -m http.server -d landing 8080   # http://localhost:8080
```

It can be published as-is (GitHub Pages, Vercel, Netlify) — all paths are relative.

## Regenerating the screenshots and clips

`assets/` is captured with Playwright from the UI running on mock data (`?scenario=…`), in light
and dark. Needs `ffmpeg` with libwebp.

```sh
npm --prefix app/ui ci && npm --prefix app/ui run build
npm --prefix app/ui run preview &        # serves the UI on http://localhost:4173
landing/scripts/capture.sh               # FLOW_UI_URL / CHROMIUM_PATH override the defaults
```

`scripts/stills.mjs` takes the stills, `scripts/motion.mjs` records the clips (switching,
arranging screens, pairing, first-run setup) as retina frame sequences with a drawn pointer, and
`capture.sh` encodes them to WebP and H.264 MP4.
