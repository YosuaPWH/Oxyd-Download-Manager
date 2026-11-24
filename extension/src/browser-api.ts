// Firefox exposes the promise-based WebExtensions API as browser.
const webExtensions = (globalThis as typeof globalThis & { browser?: typeof chrome }).browser;
export const firefox = typeof navigator !== "undefined" && /Firefox\//.test(navigator.userAgent) ? webExtensions : undefined;
export const api = firefox ?? globalThis.chrome;
