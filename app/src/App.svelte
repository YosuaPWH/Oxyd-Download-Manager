<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api } from "./lib/api";
  import * as fmt from "./lib/format";
  import { t, type Key } from "./lib/i18n.svelte";
  import { app, cancelRefresh, clock, init, isActive, matchesSearch, run, type View } from "./lib/store.svelte";
  import type { Category } from "./lib/types";
  import DetailPanel from "./components/DetailPanel.svelte";
  import DownloadTable from "./components/DownloadTable.svelte";
  import Icon from "./components/Icon.svelte";
  import Rail from "./components/Rail.svelte";
  // Settings and dialogs are loaded on demand, not in the initial bundle:
  // they are only shown after a click, while the list is shown on every
  // window open. The chunks are local files, so the first open costs one
  // frame, not a network round trip.

  onMount(() => {
    void run(init);
  });

  const tabs: { view: View; key: Key }[] = [
    { view: "all", key: "nav.all" },
    { view: "active", key: "nav.active" },
    { view: "paused", key: "nav.paused" },
    { view: "failed", key: "nav.failed" },
    { view: "completed", key: "nav.completed" },
  ];
  const categories: { value: Category; key: Key }[] = [
    { value: "video", key: "cat.video" },
    { value: "music", key: "cat.music" },
    { value: "document", key: "cat.document" },
    { value: "archive", key: "cat.archive" },
    { value: "program", key: "cat.program" },
    { value: "other", key: "cat.other" },
  ];

  // Tab counts follow the type filter and the search, like the list under them.
  const filtered = $derived(
    app.downloads.filter((d) => (!app.category || d.category === app.category) && matchesSearch(d, app.search)),
  );
  // One pass instead of one filter per tab: this recomputes on every
  // progress tick, so five full scans per tick was pure waste.
  const counts = $derived.by(() => {
    const c = { all: 0, active: 0, paused: 0, failed: 0, completed: 0 };
    for (const d of filtered) {
      c.all++;
      if (isActive(d)) c.active++;
      else if (d.status === "paused") c.paused++;
      else if (d.status === "failed" || d.status === "cancelled") c.failed++;
      else if (d.status === "completed") c.completed++;
    }
    return c;
  });
  const shown = $derived(counts[app.view]);
  // Same idea as `counts`: the header, the summary line and the footer
  // used to scan the whole list separately on every progress tick.
  // One pass produces everything they need.
  const overview = $derived.by(() => {
    const running: typeof app.downloads = [];
    const failed: typeof app.downloads = [];
    let activeCount = 0;
    let done = 0;
    for (const d of app.downloads) {
      if (d.status === "downloading") running.push(d);
      if (isActive(d)) activeCount++;
      if (d.status === "failed") failed.push(d);
      if (d.status === "completed") done++;
    }
    return { running, failed, activeCount, done };
  });
  const totalSpeed = $derived(overview.running.reduce((s, d) => s + d.speed, 0));
  const pending = $derived(app.pending[0] ?? null);
  const refreshing = $derived(app.downloads.find((d) => d.id === app.refreshing));

  /** The line under the title: what is moving, what ends next, what needs attention. */
  const summary = $derived.by(() => {
    const parts: { text: string; num?: string }[][] = [];
    const running = overview.running;
    if (running.length) {
      parts.push([{ text: `${running.length} ${t("sum.running")} ` }, { text: "", num: fmt.speed(totalSpeed) || "0 B/s" }]);
      const next = running
        .filter((d) => d.eta !== null)
        .sort((a, b) => (a.eta ?? 0) - (b.eta ?? 0))[0];
      if (next) {
        const name = next.filename.length > 36 ? `${next.filename.slice(0, 34)}…` : next.filename;
        parts.push([{ text: `${name} ${t("sum.finishes")} ` }, { text: "", num: fmt.duration(next.eta) || "0s" }]);
      }
    } else {
      const queued = overview.activeCount;
      parts.push([{ text: queued ? `${queued} ${t("sum.queued")}` : t("sum.idle") }]);
    }
    const failed = overview.failed;
    if (failed.length) {
      const retry = failed
        .map((d) => d.retryAt)
        .filter((r): r is number => r !== null)
        .sort((a, b) => a - b)[0];
      parts.push(
        retry
          ? [{ text: `${failed.length} ${t("sum.failed")}, ${t("sum.retry")} ` }, { text: "", num: fmt.duration(fmt.until(retry, clock.now)) || "0s" }]
          : [{ text: `${failed.length} ${t("sum.failed")}` }],
      );
    } else {
      const done = overview.done;
      if (done) parts.push([{ text: `${done} ${t("sum.finished")}` }]);
    }
    return parts;
  });

  // Confirmations fade out on their own.
  $effect(() => {
    if (!app.notice) return;
    const timer = setTimeout(() => (app.notice = null), 5000);
    return () => clearTimeout(timer);
  });

  let searchBox = $state<HTMLInputElement>();

  async function keydown(e: KeyboardEvent) {
    if (document.querySelector(".backdrop")) return;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "n") {
      e.preventDefault();
      app.manualUrl = "";
    } else if (mod && e.key === ",") {
      e.preventDefault();
      app.page = "settings";
    } else if (mod && e.key.toLowerCase() === "f") {
      e.preventDefault();
      app.page = "downloads";
      await tick();
      searchBox?.focus();
      searchBox?.select();
    }
  }

  async function clearCompleted() {
    await run(async () => {
      const n = await api.clearCompleted();
      app.notice = `${n} ${t("notice.cleared")}`;
    });
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
  <Rail />

  {#if app.page === "settings" && app.settings}
    {#await import("./components/SettingsPage.svelte") then { default: SettingsPage }}
      <SettingsPage />
    {/await}
  {:else}
    <div class="page">
      <main>
        <header class="title">
          <div class="heading">
            <h1>{t("page.downloads")}</h1>
            <p class="summary">
              {#each summary as part, i (i)}
                {#if i}{" · "}{/if}
                {#each part as bit, j (j)}{bit.text}{#if bit.num}<span class="num">{bit.num}</span>{/if}{/each}
              {/each}
            </p>
          </div>
          <div class="actions">
            {#if app.view === "completed" && app.downloads.some((d) => d.status === "completed")}
              <button onclick={clearCompleted} title={t("tb.clearCompletedHint")}>{t("tb.clearCompleted")}</button>
            {/if}
            <button onclick={() => run(api.resumeAll)}>{t("tb.resumeAll")}</button>
            <button onclick={() => run(api.pauseAll)}>{t("tb.pauseAll")}</button>
            <button class="primary" onclick={() => (app.manualUrl = "")} title="Ctrl+N">
              <Icon name="add" size={14} />{t("tb.add")}
            </button>
          </div>
        </header>

        {#if refreshing}
          <div class="banner" role="status">
            <Icon name="link" />
            <span class="grow">{t("refresh.waiting")} <strong>{refreshing.filename}</strong>. {t("refresh.hint")}</span>
            <button onclick={cancelRefresh}>{t("refresh.cancel")}</button>
          </div>
        {/if}

        {#if app.clipboardUrl}
          <div class="banner" role="status">
            <Icon name="clipboard" />
            <span>{t("clip.found")}</span>
            <span class="ellipsis grow">{app.clipboardUrl}</span>
            <button class="primary" onclick={downloadClipboard}>{t("clip.download")}</button>
            <button class="ghost" onclick={() => (app.clipboardUrl = null)}>{t("clip.dismiss")}</button>
          </div>
        {/if}

        <div class="work">
          <section class="list" aria-label={t("page.downloads")}>
            <div class="lines tabs" role="tablist">
              {#each tabs as tab (tab.view)}
                <button role="tab" aria-selected={app.view === tab.view} onclick={() => (app.view = tab.view)}>
                  {t(tab.key)}<span class="num count">{counts[tab.view]}</span>
                </button>
              {/each}
            </div>

            <div class="toolbar">
              <label class="search">
                <span class="sr-only">{t("search.placeholder")}</span>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
                <input
                  type="search"
                  placeholder={t("search.placeholder")}
                  bind:value={app.search}
                  bind:this={searchBox}
                  onkeydown={(e) => e.key === "Escape" && (app.search = "")}
                />
              </label>
              <select aria-label={t("nav.categories")} bind:value={app.category}>
                <option value={null}>{t("filter.allTypes")}</option>
                {#each categories as c (c.value)}
                  <option value={c.value}>{t(c.key)}</option>
                {/each}
              </select>
              <span class="grow"></span>
              <span class="muted"><span class="num">{shown}</span> {t("list.items")} · {t("list.order")}</span>
            </div>

            <DownloadTable />
          </section>

          <DetailPanel />
        </div>
      </main>

      <footer class="status">
        <span><span class="num">{overview.activeCount}</span> {t("status.active")} · <span class="num">{app.downloads.length}</span> {t("status.total")}</span>
        <span>
          ↓ <span class="num">{fmt.speed(totalSpeed) || "0 B/s"}</span> ·
          {#if app.settings?.speedLimit}{t("status.limit")} <span class="num">{fmt.speed(app.settings.speedLimit)}</span>{:else}{t("status.noLimit")}{/if}
        </span>
        <span class="grow"></span>
        <span>Oxyd <span class="num">{app.version}</span></span>
      </footer>
    </div>
  {/if}
</div>

{#if pending}
  {#key pending.key}
    {#if pending.video}
      {#await import("./components/VideoDialog.svelte") then { default: VideoDialog }}
        <VideoDialog {pending} probe={pending.video.probe} onclose={closePending} />
      {/await}
    {:else}
      {#await import("./components/NewDownloadDialog.svelte") then { default: NewDownloadDialog }}
        <NewDownloadDialog {pending} onclose={closePending} />
      {/await}
    {/if}
  {/key}
{:else if app.videoPage}
  {#await import("./components/VideoDialog.svelte") then { default: VideoDialog }}
    <VideoDialog probe={app.videoPage} onclose={() => (app.videoPage = null)} />
  {/await}
{:else if app.manualUrl !== null}
  {#await import("./components/NewDownloadDialog.svelte") then { default: NewDownloadDialog }}
    <NewDownloadDialog initialUrl={app.manualUrl} onclose={() => (app.manualUrl = null)} />
  {/await}
{/if}

{#if app.removing}
  {#await import("./components/RemoveDialog.svelte") then { default: RemoveDialog }}
    <RemoveDialog download={app.removing} onclose={() => (app.removing = null)} />
  {/await}
{/if}

{#if app.notice}
  <div class="toast" role="status">
    <span>{app.notice}</span>
    <button class="icon" aria-label="Close" onclick={() => (app.notice = null)}><Icon name="x" size={14} /></button>
  </div>
{/if}

{#if app.error}
  <div class="toast bad" role="alert">
    <span><strong>{t("err.title")}</strong><br />{app.error}</span>
    <button class="icon" aria-label="Close" onclick={() => (app.error = null)}><Icon name="x" size={14} /></button>
  </div>
{/if}

<style>
  .shell {
    display: flex;
    height: 100vh;
  }
  .page {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 28px 32px 20px;
  }
  .title {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 16px;
  }
  .heading {
    flex: 1 1 360px;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: 26px;
    line-height: 32px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .summary {
    margin: 6px 0 0;
    font-size: 14px;
    color: var(--muted);
  }
  .summary .num {
    color: var(--text);
    font-size: 13px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 8px 8px 14px;
    border-radius: var(--radius);
    background: var(--raised);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .work {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 384px;
    gap: 24px;
  }
  .list {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .tabs {
    gap: 24px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
    flex: none;
  }
  .tabs button {
    height: 40px;
  }
  .count {
    margin-left: 6px;
    color: var(--muted);
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    margin: 18px 0 6px;
  }
  .search {
    position: relative;
    width: 280px;
    max-width: 100%;
    color: var(--muted);
  }
  .search svg {
    position: absolute;
    left: 13px;
    top: 10px;
  }
  .search input {
    width: 100%;
    padding-left: 34px;
    border-radius: 999px;
  }
  .toolbar select {
    border-radius: 999px;
  }
  .status {
    display: flex;
    gap: 16px;
    align-items: center;
    height: 32px;
    padding: 0 32px;
    border-top: 1px solid var(--border);
    font-size: 12px;
    color: var(--muted);
  }
  .status .num {
    color: var(--text);
    font-size: 12px;
  }
  .toast {
    position: fixed;
    right: 20px;
    bottom: 44px;
    max-width: 420px;
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 6px 10px 14px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--float-shadow);
    z-index: 60;
  }
  .toast span {
    flex: 1;
    padding-top: 6px;
    word-break: break-word;
  }
  .toast.bad strong {
    color: var(--negative);
  }
  /* Narrow windows: the detail card goes under the list and the page scrolls as one. */
  @media (max-width: 1080px) {
    main {
      overflow-y: auto;
    }
    .work {
      flex: none;
      display: flex;
      flex-direction: column;
    }
    .work > :global(.card) {
      align-self: stretch;
      max-height: none;
    }
    .list :global(.wrap) {
      overflow-y: visible;
    }
  }
  @media (max-width: 640px) {
    main {
      padding: 20px 16px 12px;
    }
    .status {
      padding: 0 16px;
    }
  }
</style>
