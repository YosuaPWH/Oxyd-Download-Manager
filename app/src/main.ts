import { mount } from "svelte";
import "@fontsource-variable/public-sans";
import "@fontsource-variable/jetbrains-mono";
import "./app.css";
import App from "./App.svelte";

// "Follow system" is resolved here, before anything is drawn, so the window never flashes light.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

// Outside Tauri (plain browser during development) use a fake backend.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installMock } = await import("./lib/mock");
  installMock();
}

// `index.html?mini` is the mini window: the same bundle, another root component.
// `?progress=<id>` is the progress window of one download.
const query = new URLSearchParams(location.search);
const progress = query.get("progress");
const app = progress
  ? mount((await import("./ProgressWindow.svelte")).default, { target: document.getElementById("app")!, props: { id: progress } })
  : mount(query.has("mini") ? (await import("./MiniWindow.svelte")).default : App, { target: document.getElementById("app")! });

export default app;
