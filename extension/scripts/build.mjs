// Bundles the extension into build/ (load it unpacked from there).
import { cpSync, mkdirSync, rmSync } from "node:fs";
import * as esbuild from "esbuild";

const watch = process.argv.includes("--watch");
rmSync("build", { recursive: true, force: true });
mkdirSync("build", { recursive: true });
cpSync("static", "build", { recursive: true });
cpSync("src/popup/popup.html", "build/popup.html");
cpSync("src/popup/popup.css", "build/popup.css");

const common = { outdir: "build", bundle: true, target: "chrome116", logLevel: "info" };
const builds = [
  { ...common, entryPoints: { background: "src/background.ts", popup: "src/popup/popup.ts" }, format: "esm" },
  // Content scripts cannot be modules.
  { ...common, entryPoints: { content: "src/content.ts" }, format: "iife" },
];

if (watch) {
  for (const options of builds) await (await esbuild.context(options)).watch();
} else {
  await Promise.all(builds.map((options) => esbuild.build(options)));
}
