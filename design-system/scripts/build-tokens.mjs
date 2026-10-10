// Generates tokens/tokens.json from tokens/tokens.css (the single source of truth).
// Usage: node scripts/build-tokens.mjs [--check]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dir = path.dirname(fileURLToPath(import.meta.url));
const cssPath = path.join(dir, "../tokens/tokens.css");
const jsonPath = path.join(dir, "../tokens/tokens.json");
const css = fs.readFileSync(cssPath, "utf8");

/** Returns the declarations inside the first block that starts with `selector {`. */
function block(selector) {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`block not found: ${selector}`);
  const open = css.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}" && --depth === 0) return css.slice(open + 1, i);
  }
  throw new Error("unbalanced");
}

function decls(body) {
  const out = {};
  for (const m of body
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .matchAll(/--([\w-]+):\s*([^;]+);/g)) {
    out[m[1]] = m[2].replace(/\s+/g, " ").trim();
  }
  return out;
}

const light = decls(block(":root"));
const dark = decls(
  block(
    css.includes(":root[data-theme='dark'] {")
      ? ":root[data-theme='dark']"
      : ':root[data-theme="dark"]',
  ),
);

const group = (re, strip) =>
  Object.fromEntries(
    Object.entries(light)
      .filter(([k]) => re.test(k))
      .map(([k, v]) => [k.replace(strip, ""), v]),
  );

const colorKeys =
  /^(label|separator|bg-|material-|accent|focus-ring|green|orange|red|gray|scrim)/;
const shadowKeys = /^(shadow-|keycap-shadow)/;
const themed = (src) => ({
  colors: Object.fromEntries(
    Object.entries(src).filter(([k]) => colorKeys.test(k)),
  ),
  shadows: Object.fromEntries(
    Object.entries(src).filter(([k]) => shadowKeys.test(k)),
  ),
});
const lightTheme = themed(light);
const darkTheme = {
  colors: { ...lightTheme.colors, ...themed(dark).colors },
  shadows: { ...lightTheme.shadows, ...themed(dark).shadows },
};

const tokens = {
  $description:
    "Generated from tokens.css by scripts/build-tokens.mjs. Do not edit by hand.",
  font: { sans: light.font, mono: light["font-mono"] },
  typeScale: {
    largeTitle: {
      size: light["fs-large-title"],
      lineHeight: light["lh-large-title"],
      weight: light["fw-bold"],
    },
    title: {
      size: light["fs-title"],
      lineHeight: light["lh-title"],
      weight: light["fw-bold"],
    },
    title3: {
      size: light["fs-title3"],
      lineHeight: light["lh-title3"],
      weight: light["fw-semibold"],
    },
    headline: {
      size: light["fs-headline"],
      lineHeight: light["lh-body"],
      weight: light["fw-semibold"],
    },
    body: {
      size: light["fs-body"],
      lineHeight: light["lh-body"],
      weight: light["fw-regular"],
    },
    callout: {
      size: light["fs-callout"],
      lineHeight: light["lh-callout"],
      weight: light["fw-regular"],
    },
    footnote: {
      size: light["fs-footnote"],
      lineHeight: light["lh-footnote"],
      weight: light["fw-regular"],
    },
  },
  spacing: group(/^s-\d+$/, /^s-/),
  radii: group(/^r-/, /^r-/),
  motion: {
    ease: light.ease,
    durations: Object.fromEntries(
      Object.entries(group(/^dur/, /^dur-?/)).map(([k, v]) => [k || "base", v]),
    ),
  },
  themes: { light: lightTheme, dark: darkTheme },
};

const out = `${JSON.stringify(tokens, null, 2)}\n`;
if (process.argv.includes("--check")) {
  if (fs.readFileSync(jsonPath, "utf8") !== out) {
    console.error(
      "tokens.json is stale: run `npm run tokens` in design-system/",
    );
    process.exit(1);
  }
} else {
  fs.writeFileSync(jsonPath, out);
  console.log(`wrote ${path.relative(process.cwd(), jsonPath)}`);
}
