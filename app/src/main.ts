import "./bootstrap";
import { mount } from "svelte";
import App from "./App.svelte";

// The progress window shares the main HTML document and supplies the download ID.
const progress = new URLSearchParams(location.search).get("progress");
const target = document.getElementById("app")!;
const app = progress
  ? mount((await import("./ProgressWindow.svelte")).default, { target, props: { id: progress } })
  : mount(App, { target });

export default app;
