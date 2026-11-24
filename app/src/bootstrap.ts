import "@fontsource-variable/public-sans";
import "@fontsource-variable/jetbrains-mono";
import "./app.css";

document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

// The frontend can also run in a browser during development.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installMock } = await import("./lib/mock");
  installMock();
}
