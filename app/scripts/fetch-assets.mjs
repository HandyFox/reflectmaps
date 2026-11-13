// Copies map fonts and icons (Protomaps basemaps-assets, SIL OFL / CC0)
// into public/ so the app can draw labels with no internet connection.
import { execSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, rmSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const dest = join(here, "..", "public", "basemaps-assets");
const fonts = ["Noto Sans Regular", "Noto Sans Medium", "Noto Sans Italic"];

if (existsSync(join(dest, "fonts", fonts[0], "0-255.pbf")) && !process.argv.includes("--force")) {
  console.log("map fonts and icons already present");
  process.exit(0);
}

const tmp = mkdtempSync(join(tmpdir(), "basemaps-assets-"));
try {
  execSync(`git clone --depth 1 https://github.com/protomaps/basemaps-assets "${tmp}/repo"`, { stdio: "inherit" });
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(join(dest, "fonts"), { recursive: true });
  for (const f of fonts) cpSync(join(tmp, "repo", "fonts", f), join(dest, "fonts", f), { recursive: true });
  cpSync(join(tmp, "repo", "fonts", "OFL.txt"), join(dest, "fonts", "OFL.txt"));
  cpSync(join(tmp, "repo", "sprites", "v4"), join(dest, "sprites", "v4"), { recursive: true });
  console.log(`map fonts and icons copied to ${dest}`);
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
