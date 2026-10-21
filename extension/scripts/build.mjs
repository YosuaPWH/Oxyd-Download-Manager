// Bundles the extension into build/ (load it unpacked from there).
import { cpSync, mkdirSync, rmSync } from "node:fs";
import * as esbuild from "esbuild";

const watch = process.argv.includes("--watch");
rmSync("build", { recursive: true, force: true });
mkdirSync("build", { recursive: true });
cpSync("static", "build", { recursive: true });
cpSync("src/popup/popup.html", "build/popup.html");
cpSync("src/popup/popup.css", "build/popup.css");

const options = {
  entryPoints: { background: "src/background.ts", popup: "src/popup/popup.ts" },
  outdir: "build",
  bundle: true,
  format: "esm",
  target: "chrome116",
  logLevel: "info",
};

if (watch) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
} else {
  await esbuild.build(options);
}
