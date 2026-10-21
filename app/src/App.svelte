<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./lib/api";
  import { askRemove, canPause, canStart } from "./lib/actions";
  import * as fmt from "./lib/format";
  import { t } from "./lib/i18n.svelte";
  import { app, cancelRefresh, init, isActive, run, selected } from "./lib/store.svelte";
  import DetailPanel from "./components/DetailPanel.svelte";
  import DownloadTable from "./components/DownloadTable.svelte";
  import Icon from "./components/Icon.svelte";
  import NewDownloadDialog from "./components/NewDownloadDialog.svelte";
  import RemoveDialog from "./components/RemoveDialog.svelte";
  import SettingsDialog from "./components/SettingsDialog.svelte";
  import Sidebar from "./components/Sidebar.svelte";

  onMount(() => {
    void run(init);
  });

  const sel = $derived(selected());
  const active = $derived(app.downloads.filter(isActive));
  const totalSpeed = $derived(active.reduce((s, d) => s + d.speed, 0));
  const pending = $derived(app.pending[0] ?? null);
  const refreshing = $derived(app.downloads.find((d) => d.id === app.refreshing));

  // Confirmations fade out on their own.
  $effect(() => {
    if (!app.notice) return;
    const timer = setTimeout(() => (app.notice = null), 5000);
    return () => clearTimeout(timer);
  });

  function keydown(e: KeyboardEvent) {
    if (document.querySelector(".backdrop")) return;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "n") {
      e.preventDefault();
      app.manualUrl = "";
    } else if (mod && e.key === ",") {
      e.preventDefault();
      app.settingsOpen = true;
    }
  }

  function closePending() {
    app.pending.shift();
  }

  function downloadClipboard() {
    app.manualUrl = app.clipboardUrl;
    app.clipboardUrl = null;
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="shell">
  <header class="toolbar" role="toolbar" aria-label="Toolbar">
    <div class="brand"><img src="/logo.svg" alt="" width="22" height="22" /> Oxyd</div>
    <button class="primary" onclick={() => (app.manualUrl = "")} title="Ctrl+N">
      <Icon name="add" />{t("tb.add")}
    </button>
    <span class="sep"></span>
    <button disabled={!canStart(sel)} onclick={() => sel && run(() => api.start(sel.id))}>
      <Icon name="play" />{t("tb.start")}
    </button>
    <button disabled={!canPause(sel)} onclick={() => sel && run(() => api.pause(sel.id))}>
      <Icon name="pause" />{t("tb.pause")}
    </button>
    <button disabled={!sel} onclick={() => sel && askRemove(sel)} title="Delete">
      <Icon name="trash" />{t("tb.remove")}
    </button>
    <span class="sep"></span>
    <button onclick={() => run(api.resumeAll)}><Icon name="play" />{t("tb.resumeAll")}</button>
    <button onclick={() => run(api.pauseAll)}><Icon name="pause" />{t("tb.pauseAll")}</button>
    <span class="grow"></span>
    <button class="ghost" onclick={() => (app.settingsOpen = true)} title="Ctrl+,">
      <Icon name="settings" />{t("tb.settings")}
    </button>
  </header>

  {#if refreshing}
    <div class="banner refresh" role="status">
      <Icon name="link" />
      <span>{t("refresh.waiting")} <strong>{refreshing.filename}</strong>. {t("refresh.hint")}</span>
      <span class="grow"></span>
      <button class="ghost" onclick={cancelRefresh}>{t("refresh.cancel")}</button>
    </div>
  {/if}

  {#if app.clipboardUrl}
    <div class="banner" role="status">
      <Icon name="clipboard" />
      <span>{t("clip.found")}</span>
      <span class="ellipsis mono url">{app.clipboardUrl}</span>
      <button class="primary" onclick={downloadClipboard}>{t("clip.download")}</button>
      <button class="ghost" onclick={() => (app.clipboardUrl = null)}>{t("clip.dismiss")}</button>
    </div>
  {/if}

  <div class="body">
    <Sidebar />
    <main>
      <DownloadTable />
      <DetailPanel />
    </main>
  </div>

  <footer class="status">
    <span>{active.length} {t("status.active")} · {app.downloads.length} {t("status.total")}</span>
    <span class="grow"></span>
    {#if totalSpeed > 0}<span>↓ {fmt.speed(totalSpeed)}</span>{/if}
    {#if app.settings?.speedLimit}<span class="muted">· max {fmt.speed(app.settings.speedLimit)}</span>{/if}
  </footer>
</div>

{#if pending}
  {#key pending.key}
    <NewDownloadDialog {pending} onclose={closePending} />
  {/key}
{:else if app.manualUrl !== null}
  <NewDownloadDialog initialUrl={app.manualUrl} onclose={() => (app.manualUrl = null)} />
{/if}

{#if app.settingsOpen && app.settings}
  <SettingsDialog onclose={() => (app.settingsOpen = false)} />
{/if}

{#if app.removing}
  <RemoveDialog download={app.removing} onclose={() => (app.removing = null)} />
{/if}

{#if app.notice}
  <div class="toast ok" role="status">
    <span>{app.notice}</span>
    <button class="ghost" aria-label="Close" onclick={() => (app.notice = null)}>✕</button>
  </div>
{/if}

{#if app.error}
  <div class="toast" role="alert">
    <strong>{t("err.title")}</strong>
    <span>{app.error}</span>
    <button class="ghost" aria-label="Close" onclick={() => (app.error = null)}>✕</button>
  </div>
{/if}

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    font-size: 15px;
    margin-right: 10px;
  }
  .sep {
    width: 1px;
    height: 22px;
    background: var(--border);
    margin: 0 4px;
  }
  .grow {
    flex: 1;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 14px;
    background: var(--accent-soft);
    border-bottom: 1px solid var(--border);
  }
  .banner .url {
    flex: 1;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .status {
    display: flex;
    gap: 8px;
    padding: 4px 12px;
    font-size: 12px;
    color: var(--muted);
    background: var(--panel-2);
    border-top: 1px solid var(--border);
  }
  .toast {
    position: fixed;
    right: 16px;
    bottom: 36px;
    max-width: 420px;
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 12px;
    background: var(--panel);
    border: 1px solid var(--danger);
    border-radius: 8px;
    box-shadow: var(--shadow);
    z-index: 60;
  }
  .toast.ok {
    border-color: var(--ok);
  }
  .banner.refresh {
    background: color-mix(in srgb, var(--warn) 14%, var(--panel));
  }
  .toast span {
    flex: 1;
    word-break: break-word;
  }
</style>
