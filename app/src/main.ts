import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// Outside Tauri (plain browser during development) use a fake backend.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installMock } = await import("./lib/mock");
  installMock();
}

const app = mount(App, { target: document.getElementById("app")! });

export default app;
