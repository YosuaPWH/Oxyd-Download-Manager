// Bundles unpacked Chromium and Firefox extensions side by side.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import * as esbuild from "esbuild";

const watch = process.argv.includes("--watch");
const manifest = JSON.parse(readFileSync("static/manifest.json", "utf8"));

const builds = [];
for (const [outdir, target] of [["build", "chrome116"], ["build-firefox", "firefox140"]]) {
  const firefox = outdir === "build-firefox";
  rmSync(outdir, { recursive: true, force: true });
  mkdirSync(outdir, { recursive: true });
  cpSync("static", outdir, { recursive: true });
  cpSync("src/popup/popup.html", `${outdir}/popup.html`);
  cpSync("src/popup/popup.css", `${outdir}/popup.css`);

  if (firefox) {
    const { key, minimum_chrome_version, version_name, ...shared } = manifest;
    writeFileSync(`${outdir}/manifest.json`, JSON.stringify({
      ...shared,
      description: "Send Firefox downloads and videos to Oxyd Download Manager.",
      background: { scripts: ["background.js"] },
      browser_specific_settings: { gecko: { id: "oxyd@yosuapwh.github.io", strict_min_version: "140.0" } },
    }, null, 2) + "\n");
  }
  const common = { outdir, bundle: true, target, logLevel: "info" };
  builds.push(
    { ...common, entryPoints: { background: "src/background.ts" }, format: firefox ? "iife" : "esm" },
    { ...common, entryPoints: { content: "src/content.ts", popup: "src/popup/popup.ts" }, format: "iife" },
  );
}

if (watch) {
  for (const options of builds) await (await esbuild.context(options)).watch();
} else {
  await Promise.all(builds.map((options) => esbuild.build(options)));
}
