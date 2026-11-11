<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api } from "./lib/api";
  import { askRemove, canPause, canStart, pauseSelected, startSelected } from "./lib/actions";
  import { parseBatch } from "./lib/batch";
  import * as fmt from "./lib/format";
  import { t, type Key } from "./lib/i18n.svelte";
  import {
    app,
    cancelRefresh,
    clock,
    dismissJob,
    init,
    inScope,
    isActive,
    run,
    saveLayout,
    selectedDownloads,
    setGlobalLimit,
    type View,
  } from "./lib/store.svelte";
  import DownloadTable from "./components/DownloadTable.svelte";
  import Icon from "./components/Icon.svelte";
  import Inspector from "./components/Inspector.svelte";
  import Sidebar from "./components/Sidebar.svelte";
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

  // One pass instead of one filter per tab: this recomputes on every
  // progress tick, so five full scans per tick was pure waste.
  const counts = $derived.by(() => {
    const c = { all: 0, active: 0, paused: 0, failed: 0, completed: 0, bytes: 0 };
    for (const d of app.downloads) {
      if (!inScope(d)) continue;
      c.all++;
      c.bytes += d.totalSize ?? d.downloaded;
      if (isActive(d)) c.active++;
      else if (d.status === "paused") c.paused++;
      else if (d.status === "failed" || d.status === "cancelled") c.failed++;
      else if (d.status === "completed") c.completed++;
    }
    return c;
  });
  // Same idea as `counts`: the summary line and the status bar read one pass.
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

  const heading = $derived(
    app.queue
      ? app.queues.find((q) => q.id === app.queue)?.name || t("queue.main")
      : app.category
      ? t(`cat.${app.category}` as Key)
      : t(app.library === "all" ? "side.all" : app.library === "finished" ? "nav.completed" : "nav.queued"),
  );

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
    }
    return parts;
  });

  // Toolbar actions follow the selection.
  const picked = $derived(selectedDownloads());
  const canStartAny = $derived(picked.some(canStart));
  const canPauseAny = $derived(picked.some(canPause));

  // Confirmations fade out on their own.
  $effect(() => {
    if (!app.notice) return;
    const timer = setTimeout(() => (app.notice = null), 5000);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    if (!app.finished) return;
    const timer = setTimeout(() => (app.finished = null), 8000);
    return () => clearTimeout(timer);
  });

  let searchBox = $state<HTMLInputElement>();

  function toggleInspector() {
    app.inspector = !app.inspector;
    saveLayout();
  }

  async function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") limitMenu = false;
    if (document.querySelector(".backdrop")) return;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (mod && key === "n") {
      e.preventDefault();
      app.manualUrl = "";
    } else if (mod && key === "b") {
      e.preventDefault();
      app.batch = "";
    } else if (mod && key === "g") {
      e.preventDefault();
      app.grabber = true;
    } else if (mod && key === "i") {
      e.preventDefault();
      toggleInspector();
    } else if (mod && e.key === ",") {
      e.preventDefault();
      app.page = "settings";
    } else if (mod && key === "f") {
      e.preventDefault();
      app.page = "downloads";
      await tick();
      searchBox?.focus();
      searchBox?.select();
    }
  }

  async function toggleMini() {
    await run(async () => {
      app.mini = await api.toggleMini();
    });
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

  /** Links dropped on or pasted into the window: one opens "New download", several the batch. */
  function offer(text: string): boolean {
    const magnet = text.split(/\s+/).find((w) => /^magnet:\?/i.test(w));
    if (magnet) {
      app.page = "downloads";
      app.manualUrl = magnet;
      return true;
    }
    const { urls } = parseBatch(text);
    if (!urls.length) return false;
    app.page = "downloads";
    if (urls.length === 1) app.manualUrl = urls[0];
    else app.batch = urls.join("\n");
    return true;
  }

  // Dropping links. `dragenter`/`dragleave` fire for every element crossed, so depth is counted.
  let dragDepth = $state(0);
  const carriesLinks = (e: DragEvent) =>
    !!e.dataTransfer && [...e.dataTransfer.types].some((x) => x === "text/uri-list" || x === "text/plain");
  const dialogOpen = () => !!document.querySelector(".backdrop");

  function dragenter(e: DragEvent) {
    if (carriesLinks(e) && !dialogOpen()) dragDepth++;
  }
  function dragover(e: DragEvent) {
    if (dragDepth) e.preventDefault();
  }
  function dragleave() {
    if (dragDepth) dragDepth--;
  }
  function drop(e: DragEvent) {
    if (!dragDepth) return;
    e.preventDefault();
    dragDepth = 0;
    const text = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "";
    // A uri-list may carry "# comment" lines.
    if (!offer(text.replace(/^#.*$/gm, ""))) app.notice = t("drop.none");
  }

  function paste(e: ClipboardEvent) {
    const el = e.target as HTMLElement | null;
    if (dialogOpen() || el?.closest("input, textarea, [contenteditable]")) return;
    if (offer(e.clipboardData?.getData("text/plain") ?? "")) e.preventDefault();
  }

  // The speed limit chip in the status bar.
  let limitMenu = $state(false);
  const MB = 1024 * 1024;
  const presets = [256 * 1024, 512 * 1024, MB, 2 * MB, 5 * MB, 10 * MB];
  const limit = $derived(app.settings?.speedLimit ?? null);

  function pickLimit(v: number | null) {
    limitMenu = false;
    void setGlobalLimit(v);
  }

  const DAYS: Key[] = ["day.0", "day.1", "day.2", "day.3", "day.4", "day.5", "day.6"];
  /** "Mon–Fri 01:00" for the status bar; null when no schedule runs. */
  const schedule = $derived.by(() => {
    const s = app.settings?.schedule;
    if (!s?.enabled || !(s.start || s.stop)) return null;
    const days = [...s.days].sort();
    const run = days.length > 1 && days.every((d, i) => i === 0 || d === days[i - 1] + 1);
    const when =
      !days.length || days.length === 7
        ? t("sched.daily")
        : run
          ? `${t(DAYS[days[0]])}–${t(DAYS[days[days.length - 1]])}`
          : days.map((d) => t(DAYS[d])).join(", ");
    return { when, time: [s.start, s.stop].filter(Boolean).join("–") };
  });
</script>

<svelte:window
  onkeydown={keydown}
  onclick={() => (limitMenu = false)}
  ondragenter={dragenter}
  ondragover={dragover}
  ondragleave={dragleave}
  ondrop={drop}
  onpaste={paste}
/>

<div class="shell">
  <Sidebar />

  <div class="sheet">
    {#if app.page === "settings" && app.settings}
      {#await import("./components/SettingsPage.svelte") then { default: SettingsPage }}
        <SettingsPage />
      {/await}
    {:else}
      <div class="toolbar" role="toolbar" aria-label={t("page.downloads")}>
        <button class="primary" onclick={() => (app.manualUrl = "")} title="Ctrl+N">
          <Icon name="add" size={14} />{t("tb.add")}
        </button>
        <button onclick={() => (app.batch = "")} title="{t('tb.batchHint')} (Ctrl+B)">
          <Icon name="layers" size={14} /><span class="wide">{t("tb.batch")}</span>
        </button>
        <span class="sep"></span>
        <button class="icon" title={t("tb.resumeSel")} aria-label={t("tb.resumeSel")} disabled={!canStartAny} onclick={startSelected}><Icon name="play" /></button>
        <button class="icon" title={t("tb.pauseSel")} aria-label={t("tb.pauseSel")} disabled={!canPauseAny} onclick={pauseSelected}><Icon name="pause" /></button>
        <button class="icon" title="{t('tb.removeSel')} (Del)" aria-label={t("tb.removeSel")} disabled={!picked.length} onclick={() => askRemove(picked)}><Icon name="trash" /></button>
        <span class="sep"></span>
        <button class="icon" title={t("tb.resumeAll")} aria-label={t("tb.resumeAll")} onclick={() => run(api.resumeAll)}><Icon name="playAll" /></button>
        <button class="icon" title={t("tb.pauseAll")} aria-label={t("tb.pauseAll")} onclick={() => run(api.pauseAll)}><Icon name="pauseAll" /></button>
        <button class="icon" title={t("tb.clearCompletedHint")} aria-label={t("tb.clearCompleted")} disabled={!overview.done} onclick={clearCompleted}><Icon name="sweep" /></button>
        <span class="sep"></span>
        <button class="icon" title="{t('grab.title')} (Ctrl+G)" aria-label={t("grab.title")} onclick={() => (app.grabber = true)}><Icon name="globe" /></button>
        <button class="icon" title={t("mini.toggle")} aria-label={t("mini.toggle")} aria-pressed={app.mini} onclick={toggleMini}><Icon name="pip" /></button>
        <span class="grow"></span>
        <label class="search">
          <span class="sr-only">{t("search.placeholder")}</span>
          <Icon name="search" size={14} />
          <input
            type="search"
            placeholder={t("search.placeholder")}
            bind:value={app.search}
            bind:this={searchBox}
            onkeydown={(e) => e.key === "Escape" && (app.search = "")}
          />
          {#if !app.search}<kbd>Ctrl F</kbd>{/if}
        </label>
        <button class="icon" title="{t('insp.title')} (Ctrl+I)" aria-label={t("insp.title")} aria-pressed={app.inspector} onclick={toggleInspector}><Icon name="panel" /></button>
      </div>

      <header class="title">
        <div class="heading">
          <h1>{heading}</h1>
          <p class="summary">
            <span class="num">{counts.all}</span> {t("list.items")} · <span class="num">{fmt.bytes(counts.bytes)}</span>
            {#each summary as part, i (i)}
              {" · "}{#each part as bit, j (j)}{bit.text}{#if bit.num}<span class="num">{bit.num}</span>{/if}{/each}
            {/each}
          </p>
        </div>
        {#if picked.length > 1}
          <span class="pill accent"><span class="num">{picked.length}</span>&nbsp;{t("sel.count")}</span>
        {/if}
        <div class="lines" role="tablist" aria-label={t("nav.status")}>
          {#each tabs as tab (tab.view)}
            <button role="tab" aria-selected={app.view === tab.view} onclick={() => (app.view = tab.view)}>
              {t(tab.key)}<span class="num count">{counts[tab.view]}</span>
            </button>
          {/each}
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
          <span class="ellipsis grow mono">{app.clipboardUrl}</span>
          <button class="primary" onclick={downloadClipboard}>{t("clip.download")}</button>
          <button class="ghost" onclick={() => (app.clipboardUrl = null)}>{t("clip.dismiss")}</button>
        </div>
      {/if}

      <DownloadTable />

      {#if app.inspector}
        <Inspector />
      {/if}
    {/if}

    <footer class="status">
      <span title={t("side.totalSpeed")}>↓ <span class="num">{fmt.speed(totalSpeed) || "0 B/s"}</span></span>
      <span><span class="num">{overview.activeCount}</span> {t("status.active")} · <span class="num">{app.downloads.length}</span> {t("status.total")}</span>
      {#if schedule}
        <span class="sched" title={t("set.schedule")}><Icon name="calendar" size={12} />{schedule.when} <span class="num">{schedule.time}</span></span>
      {/if}
      <span class="grow"></span>
      <span class="limit">
        <button
          class="chip"
          class:on={!!limit}
          aria-haspopup="menu"
          aria-expanded={limitMenu}
          title={t("set.speedLimit")}
          onclick={(e) => (e.stopPropagation(), (limitMenu = !limitMenu))}
        >
          <Icon name="gauge" size={12} />
          {#if limit}{t("status.limit")} <span class="num">{fmt.speed(limit)}</span>{:else}{t("status.noLimit")}{/if}
        </button>
        {#if limitMenu}
          <div class="menu up" role="menu">
            <button role="menuitemradio" aria-checked={!limit} onclick={() => pickLimit(null)}>{t("limit.off")}</button>
            <hr />
            {#each presets as p (p)}
              <button role="menuitemradio" aria-checked={limit === p} onclick={() => pickLimit(p)}><span class="num">{fmt.speed(p)}</span></button>
            {/each}
            {#if limit && !presets.includes(limit)}
              <button role="menuitemradio" aria-checked="true"><span class="num">{fmt.speed(limit)}</span></button>
            {:else if !limit && !presets.includes(app.lastLimit)}
              <button role="menuitemradio" aria-checked="false" onclick={() => pickLimit(app.lastLimit)}><span class="num">{fmt.speed(app.lastLimit)}</span></button>
            {/if}
          </div>
        {/if}
      </span>
      <span>Oxyd <span class="num">{app.version}</span></span>
    </footer>
  </div>
</div>

{#if dragDepth > 0}
  <div class="dropzone" aria-hidden="true">
    <div>
      <Icon name="active" size={28} />
      <strong>{t("drop.title")}</strong>
      <span class="muted">{t("drop.body")}</span>
    </div>
  </div>
{/if}

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
{:else if app.batch !== null}
  {#await import("./components/BatchDialog.svelte") then { default: BatchDialog }}
    <BatchDialog initial={app.batch} onclose={() => (app.batch = null)} />
  {/await}
{/if}

{#if app.grabber}
  {#await import("./components/GrabberDialog.svelte") then { default: GrabberDialog }}
    <GrabberDialog onclose={() => (app.grabber = false)} />
  {/await}
{/if}

{#if app.queueEdit}
  {#await import("./components/QueueDialog.svelte") then { default: QueueDialog }}
    {#key app.queueEdit.id}
      <QueueDialog queue={app.queueEdit} onclose={() => (app.queueEdit = null)} />
    {/key}
  {/await}
{/if}

{#if app.tools}
  {#await import("./components/ToolsDialog.svelte") then { default: ToolsDialog }}
    <ToolsDialog downloads={app.tools} onclose={() => (app.tools = null)} />
  {/await}
{/if}

{#if app.removing}
  {#await import("./components/RemoveDialog.svelte") then { default: RemoveDialog }}
    <RemoveDialog downloads={app.removing} onclose={() => (app.removing = null)} />
  {/await}
{/if}

<div class="toasts">
  {#each app.jobs as job (job.id)}
    <div class="toast job" class:bad={job.state === "failed"} role="status">
      <span class="ring" class:work={job.state === "running" || job.state === "waiting"}>
        <Icon name={job.state === "done" ? "completed" : job.state === "failed" ? "alert" : "wrench"} size={15} />
      </span>
      <span class="text">
        <strong class="ellipsis">{job.state === "done" && job.output ? job.output.split(/[\\/]/).pop() : job.name}</strong>
        <span class="muted">
          {t(`tools.${job.kind}` as Key)} · {t(`job.${job.state}` as Key)}{#if job.total > 1}
            · <span class="num">{Math.min(job.done + 1, job.total)}/{job.total}</span>{/if}
        </span>
        {#if job.state === "running"}
          <span class="jobtrack"><span style:width="{job.progress * 100}%"></span></span>
        {:else if job.error}
          <span class="err">{job.error}</span>
        {/if}
      </span>
      {#if job.state === "running" || job.state === "waiting"}
        <button onclick={() => api.cancelTool(job.id)}>{t("new.cancel")}</button>
      {:else}
        <button class="icon" aria-label={t("clip.dismiss")} onclick={() => dismissJob(job.id)}><Icon name="x" size={14} /></button>
      {/if}
    </div>
  {/each}

  {#if app.finished}
    {@const f = app.finished}
    <div class="toast done" role="status">
      <span class="ring"><Icon name="completed" size={16} /></span>
      <span class="text">
        <strong class="ellipsis">{f.filename}</strong>
        <span class="muted">{t("toast.finished")}{#if f.totalSize} · <span class="num">{fmt.bytes(f.totalSize)}</span>{/if}</span>
      </span>
      <button onclick={() => ((app.finished = null), void run(() => api.openFile(f.id)))}>{t("toast.open")}</button>
      <button class="icon" title={t("act.folder")} aria-label={t("act.folder")} onclick={() => ((app.finished = null), void run(() => api.openFolder(f.id)))}><Icon name="folder" /></button>
      <button class="icon" aria-label={t("clip.dismiss")} onclick={() => (app.finished = null)}><Icon name="x" size={14} /></button>
    </div>
  {/if}

  {#if app.notice}
    <div class="toast" role="status">
      <span class="text">{app.notice}</span>
      <button class="icon" aria-label={t("clip.dismiss")} onclick={() => (app.notice = null)}><Icon name="x" size={14} /></button>
    </div>
  {/if}

  {#if app.error}
    <div class="toast bad" role="alert">
      <span class="text"><strong>{t("err.title")}</strong><span>{app.error}</span></span>
      <button class="icon" aria-label={t("clip.dismiss")} onclick={() => (app.error = null)}><Icon name="x" size={14} /></button>
    </div>
  {/if}
</div>

<style>
  /* The window: a tinted panel beside the sheet that holds the data. The accent lights the
     top left corner; that light is all the "glass" has to refract. */
  .shell {
    display: flex;
    gap: 8px;
    height: 100vh;
    padding: 8px;
    background:
      radial-gradient(560px 320px at 0% 0%, var(--accent-soft), transparent 70%),
      radial-gradient(420px 260px at 0% 100%, var(--accent-softer), transparent 70%), var(--ground);
  }
  :global(.no-glass) .shell {
    background: var(--ground);
  }
  .sheet {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--sheet-shadow);
    overflow: hidden;
  }
  .toolbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 50px;
    padding: 0 12px;
    border-bottom: 1px solid var(--rule);
  }
  .toolbar > button:not(.icon) {
    margin-right: 4px;
  }
  .sep {
    flex: none;
    width: 1px;
    height: 18px;
    margin: 0 6px;
    background: var(--border);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    width: 250px;
    min-width: 120px;
    flex-shrink: 1;
    margin-right: 4px;
    color: var(--faint);
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    pointer-events: none;
  }
  .search input {
    width: 100%;
    padding-left: 30px;
    border-color: var(--rule);
  }
  .search kbd {
    position: absolute;
    right: 8px;
    pointer-events: none;
  }
  .title {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 16px;
    padding: 12px 16px 10px;
  }
  .heading {
    flex: 1 1 280px;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: 18px;
    line-height: 24px;
    font-weight: 600;
    letter-spacing: -0.015em;
  }
  .summary {
    margin: 1px 0 0;
    font-size: 12px;
    line-height: 18px;
    color: var(--faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .summary .num {
    color: var(--muted);
    font-size: 11.5px;
  }
  .count {
    margin-left: 6px;
    font-size: 11px;
    color: var(--faint);
  }
  .banner {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0 12px 8px;
    padding: 6px 6px 6px 12px;
    border-radius: var(--radius);
    background: var(--accent-softer);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  .status {
    flex: none;
    display: flex;
    gap: 16px;
    align-items: center;
    height: 28px;
    padding: 0 14px;
    border-top: 1px solid var(--rule);
    background: var(--sunken);
    font-size: 11.5px;
    color: var(--faint);
    white-space: nowrap;
  }
  .status .num {
    color: var(--text);
    font-size: 11.5px;
  }
  .sched {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .limit {
    position: relative;
  }
  .chip {
    height: 20px;
    padding: 0 8px;
    gap: 5px;
    border-color: var(--border);
    border-radius: 6px;
    font-size: 11.5px;
    font-weight: 400;
    color: var(--muted);
  }
  .chip.on {
    color: var(--warning);
    border-color: color-mix(in srgb, var(--warning) 45%, transparent);
    background: var(--warn-bg);
  }
  .chip.on .num {
    color: inherit;
  }
  .menu.up {
    position: absolute;
    right: 0;
    bottom: 26px;
    min-width: 150px;
  }
  .menu.up button[aria-checked="true"] {
    color: var(--accent);
    font-weight: 600;
  }
  .menu.up .num {
    color: inherit;
    font-size: 12px;
  }
  .dropzone {
    position: fixed;
    inset: 8px;
    z-index: 45;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--ground) 82%, transparent);
    backdrop-filter: blur(6px);
    pointer-events: none;
  }
  .dropzone div {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: var(--accent);
  }
  .dropzone strong {
    font-size: 16px;
    color: var(--text);
  }
  .toasts {
    position: fixed;
    right: 20px;
    bottom: 44px;
    z-index: 60;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    max-width: 440px;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 6px 8px 14px;
    background: var(--raised);
    border-radius: 10px;
    box-shadow: var(--float-shadow);
  }
  .toast .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    word-break: break-word;
  }
  .toast.done {
    padding-left: 10px;
    gap: 10px;
    min-width: 320px;
  }
  .ring {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    color: var(--positive);
    background: var(--pos-bg);
  }
  .toast.job {
    padding-left: 10px;
    gap: 10px;
    width: 360px;
  }
  .ring.work {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .toast.bad .ring {
    color: var(--negative);
    background: var(--neg-bg);
  }
  .jobtrack {
    display: block;
    height: 3px;
    margin-top: 5px;
    border-radius: 2px;
    background: var(--track);
    overflow: hidden;
  }
  .jobtrack span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s linear;
  }
  .err {
    font-size: 12px;
    color: var(--negative);
  }
  .toast.bad strong {
    color: var(--negative);
  }
  @media (max-width: 1000px) {
    .wide {
      display: none;
    }
  }
</style>
