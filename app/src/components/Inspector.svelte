<script lang="ts">
  import { api } from "../lib/api";
  import { canPause, canStart, hostOf } from "../lib/actions";
  import * as fmt from "../lib/format";
  import { detailText, errorText, t, type Key } from "../lib/i18n.svelte";
  import { app, beginRefresh, clock, run, samplesOf, saveLayout, selected, type InspectorTab } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import SegmentBar from "./SegmentBar.svelte";
  import SpeedChart from "./SpeedChart.svelte";

  const d = $derived(selected());

  let limit = $state("");
  let newUrl = $state("");
  let lastId = "";

  $effect(() => {
    // Reset the inputs when another download gets selected.
    if (d && d.id !== lastId) {
      lastId = d.id;
      limit = fmt.rateInput(d.speedLimit);
      newUrl = "";
    }
  });

  const samples = $derived(d ? samplesOf(d.id) : []);

  function applyLimit() {
    if (!d) return;
    const id = d.id;
    void run(() => api.setLimit(id, fmt.parseRate(limit)));
  }

  function applyUrl() {
    if (!d || !newUrl.trim()) return;
    const id = d.id;
    const url = newUrl.trim();
    void run(async () => {
      await api.refreshUrl(id, url);
      newUrl = "";
    });
  }

  function tab(v: InspectorTab) {
    app.inspectorTab = v;
    saveLayout();
  }

  function close() {
    app.inspector = false;
    saveLayout();
  }

  const tabs: { value: InspectorTab; key: Key }[] = [
    { value: "connections", key: "insp.connections" },
    { value: "speed", key: "insp.speed" },
    { value: "info", key: "insp.info" },
  ];

  // Running downloads too: a dead link is usually still being retried.
  // Videos find their links again on the page by themselves.
  const canRefresh = $derived(!!d && !d.media && d.status !== "completed" && d.status !== "cancelled");
  const pct = $derived(d ? fmt.percent(d.downloaded, d.totalSize) : null);
  const moving = $derived(d?.status === "downloading");

  /** One lane per connection: its share of the file and how far it got. */
  const lanes = $derived.by(() => {
    if (!d) return [];
    // Unknown-length streams use u64::MAX as their end.
    const size = d.totalSize ?? d.segments.reduce((m, s) => Math.max(m, s.start + s.downloaded), 0);
    return d.segments.map((s, i) => {
      const end = Math.min(s.end, Math.max(size - 1, s.start));
      const len = Math.max(end - s.start + 1, 0);
      const p = len ? Math.min(100, (s.downloaded / len) * 100) : 100;
      return { i, start: s.start, end, p, left: Math.max(0, len - s.downloaded) };
    });
  });
  const busy = $derived(lanes.filter((l) => l.p < 100).length);

  const stats = $derived.by(() => {
    const avg = samples.length ? samples.reduce((a, b) => a + b, 0) / samples.length : 0;
    return { avg, peak: samples.length ? Math.max(...samples) : 0 };
  });
</script>

<section class="insp" aria-label={t("insp.title")}>
  <header>
    <div class="tabs" role="tablist">
      {#each tabs as x (x.value)}
        <button role="tab" aria-selected={app.inspectorTab === x.value} onclick={() => tab(x.value)}>{t(x.key)}</button>
      {/each}
    </div>
    {#if d}
      <span class="ellipsis file" title={d.filename || d.url}>{d.filename || d.url}</span>
      {#if moving}<span class="pill accent"><span class="num">{d.activeConnections}</span>&nbsp;{t(d.kind === "torrent" ? "insp.peers" : "insp.conn")}</span>{/if}
      {#if canPause(d)}
        <button class="icon" title={t("act.pause")} aria-label={t("act.pause")} onclick={() => d && run(() => api.pause(d.id))}><Icon name="pause" /></button>
      {:else if canStart(d)}
        <button class="icon" title={t("act.resume")} aria-label={t("act.resume")} onclick={() => d && run(() => api.start(d.id))}><Icon name="play" /></button>
      {/if}
      {#if d.status === "completed"}
        <button class="icon" title={t("act.open")} aria-label={t("act.open")} onclick={() => d && run(() => api.openFile(d.id))}><Icon name="file" /></button>
      {/if}
      <button class="icon" title={t("act.folder")} aria-label={t("act.folder")} onclick={() => d && run(() => api.openFolder(d.id))}><Icon name="folder" /></button>
      <button class="icon" title={t("act.copy")} aria-label={t("act.copy")} onclick={() => d && run(() => api.copy(d.url))}><Icon name="copy" /></button>
    {:else}
      <span class="file"></span>
    {/if}
    <button class="icon" title="{t('insp.hide')} (Ctrl+I)" aria-label={t("insp.hide")} onclick={close}><Icon name="chevronDown" /></button>
  </header>

  <div class="body">
    {#if !d}
      <p class="muted none">{t("detail.none")}</p>
    {:else if app.inspectorTab === "connections"}
      {#if d.error}
        <div class="error" role="alert">
          <span class="pill bad">{t("st.failed")}</span>
          <span>{errorText(d.errorCode) ?? d.error}</span>
          {#if d.status === "failed" && d.retryAt}
            <span class="muted">{t("retry.in")} <span class="num">{fmt.duration(fmt.until(d.retryAt, clock.now)) || "0s"}</span></span>
          {/if}
        </div>
      {/if}
      <div class="summary">
        <span class="num figure">{fmt.bytes(d.downloaded)}{#if d.totalSize !== null}<span class="of">/ {fmt.bytes(d.totalSize)}</span>{/if}</span>
        <span class="muted">
          {#if pct !== null}<span class="num ink">{pct.toFixed(1)}%</span>{:else}{t("detail.unknownSize")}{/if}
          {#if moving}
            · <span class="num ink">{fmt.speed(d.speed) || "0 B/s"}</span>
            {#if d.eta !== null}· <span class="num ink">{fmt.duration(d.eta)}</span> {t("detail.left")}{/if}
          {:else}
            · {d.detail && d.status !== "completed" ? detailText(d.detail) : t(`st.${d.status}` as Key)}
          {/if}
        </span>
        <span class="grow"></span>
        {#if lanes.length && moving}
          <span class="faint"><span class="num">{busy}</span> {t("detail.of")} <span class="num">{lanes.length}</span> {t("detail.running")}</span>
        {:else if lanes.length}
          <span class="faint"><span class="num">{lanes.length}</span> {t("detail.segments").toLowerCase()}</span>
        {/if}
      </div>
      {#if d.segments.length || d.status === "completed"}
        <SegmentBar segments={d.segments} total={d.totalSize} downloaded={d.downloaded} />
      {/if}
      {#if lanes.length}
        <div class="lanes">
          {#each lanes as l (l.i)}
            <div class="lane" style:--c="var(--lane-{(l.i % 8) + 1})">
              <span class="num faint">{String(l.i + 1).padStart(2, "0")}</span>
              <div class="track"><div class="fill" class:done={l.p >= 100} style:width="{l.p}%"></div></div>
              <span class="num faint range">{fmt.bytes(l.start)}–{fmt.bytes(l.end + 1)}</span>
              <span class="num left">{l.p >= 100 ? t("detail.done") : fmt.bytes(l.left)}</span>
            </div>
          {/each}
        </div>
      {:else if d.status !== "completed"}
        <p class="faint none">{t("insp.noLanes")}</p>
      {/if}
    {:else if app.inspectorTab === "speed"}
      <div class="speed">
        <div class="chart"><SpeedChart {samples} title={t("detail.speedMinute")} /></div>
        <dl class="tiles">
          <div><dt>{t("insp.now")}</dt><dd class="num">{moving ? fmt.speed(d.speed) || "0 B/s" : "—"}</dd></div>
          <div><dt>{t("insp.average")}</dt><dd class="num">{fmt.speed(stats.avg) || "—"}</dd></div>
          <div><dt>{t("side.peak")}</dt><dd class="num">{fmt.speed(stats.peak) || "—"}</dd></div>
          <div><dt>{t("col.eta")}</dt><dd class="num">{moving && d.eta !== null ? fmt.duration(d.eta) : "—"}</dd></div>
        </dl>
      </div>
    {:else}
      <div class="info">
        <dl class="facts">
          <dt>{t("detail.path")}</dt>
          <dd title={d.savePath}>{d.savePath || d.dir}</dd>
          <dt>{t("detail.url")}</dt>
          <dd class="ellipsis" title={d.finalUrl ?? d.url}>{d.url}</dd>
          <dt>{t("insp.source")}</dt>
          <dd>{d.kind === "torrent" ? "BitTorrent" : hostOf(d) || "—"} · {t(`cat.${d.category}` as Key)}</dd>
          {#if app.queues.length > 1}
            <dt>{t("queue.label")}</dt>
            <dd>{app.queues.find((q) => q.id === d.queue)?.name || t("queue.main")}</dd>
          {/if}
          <dt>{t("detail.resumable")}</dt>
          <dd>{d.resumable ? t("yes") : t("no")} · <span class="num">{d.connections}</span> {t("detail.connections").toLowerCase()}</dd>
          <dt>{t("detail.added")}</dt>
          <dd>{fmt.date(d.createdAt)}</dd>
          {#if d.completedAt}
            <dt>{t("detail.completedAt")}</dt>
            <dd>{fmt.date(d.completedAt)}</dd>
          {/if}
          {#if d.checksum}
            <dt>{t("new.checksum")}</dt>
            <dd class="ellipsis mono" title={d.checksum}>{d.checksum}</dd>
          {/if}
          {#if d.error}
            <dt>{t("detail.error")}</dt>
            <dd class="bad">{errorText(d.errorCode) ?? d.error}</dd>
          {/if}
        </dl>
        {#if d.status !== "completed"}
          <div class="forms">
            <form onsubmit={(e) => (e.preventDefault(), applyLimit())}>
              <label for="limit">{t("detail.limit")}</label>
              <div class="field">
                <input id="limit" bind:value={limit} placeholder={t("detail.limitHint")} />
                <button type="submit">{t("detail.apply")}</button>
              </div>
            </form>
            {#if canRefresh}
              <form onsubmit={(e) => (e.preventDefault(), applyUrl())}>
                <label for="newurl">{t("act.refresh")}</label>
                <div class="field">
                  <input id="newurl" bind:value={newUrl} placeholder={t("detail.newUrl")} />
                  <button type="submit" disabled={!newUrl.trim()}>{t("detail.apply")}</button>
                </div>
                <button type="button" class="from" onclick={() => d && beginRefresh(d)} title={t("refresh.hint")}>
                  <Icon name="link" size={14} />{t("refresh.fromBrowser")}
                </button>
              </form>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .insp {
    flex: none;
    display: flex;
    flex-direction: column;
    height: 236px;
    border-top: 1px solid var(--border);
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 36px;
    padding: 0 8px 0 16px;
    border-bottom: 1px solid var(--rule);
  }
  .tabs {
    align-self: stretch;
    display: flex;
    gap: 18px;
    margin-right: 12px;
  }
  .tabs button {
    height: 100%;
    padding: 0;
    border: 0;
    border-radius: 0;
    font-size: 12.5px;
    color: var(--faint);
  }
  .tabs button:hover:not(:disabled) {
    background: none;
    color: var(--text);
  }
  .tabs button[aria-selected="true"] {
    color: var(--text);
    font-weight: 600;
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .file {
    flex: 1;
    min-width: 0;
    text-align: right;
    font-size: 12.5px;
    color: var(--muted);
    margin-right: 6px;
  }
  header .icon {
    width: 28px;
    height: 28px;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 16px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .none {
    margin: 0;
  }
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 12px;
  }
  .figure {
    font-size: 18px;
    line-height: 24px;
    letter-spacing: -0.02em;
    font-weight: 500;
  }
  .of {
    font-size: 12.5px;
    letter-spacing: 0;
    font-weight: 400;
    margin-left: 6px;
    color: var(--muted);
  }
  .ink {
    color: var(--text);
  }
  .grow {
    flex: 1;
  }
  .error {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
  }
  .lanes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
    gap: 6px 28px;
  }
  .lane {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) 128px 62px;
    gap: 10px;
    align-items: center;
  }
  .lane .num {
    font-size: 11px;
    white-space: nowrap;
  }
  .lane .track {
    height: 4px;
    border-radius: 2px;
    background: var(--track);
    overflow: hidden;
  }
  .lane .fill {
    height: 100%;
    border-radius: 2px;
    background: var(--c);
    transition: width 0.25s linear;
  }
  .lane .fill.done {
    background: var(--done);
  }
  .range,
  .left {
    text-align: right;
  }
  .left {
    color: var(--muted);
  }
  .speed {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 190px;
    gap: 24px;
    align-items: start;
  }
  .chart {
    min-width: 0;
    max-width: 640px;
  }
  .tiles {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin: 0;
  }
  .tiles div {
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--sunken);
    box-shadow: inset 0 0 0 1px var(--rule);
  }
  .tiles dt {
    font-size: 11px;
    color: var(--faint);
  }
  .tiles dd {
    margin: 0;
    font-size: 13px;
  }
  .info {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(260px, 1fr);
    gap: 28px;
  }
  .facts {
    display: grid;
    grid-template-columns: 110px minmax(0, 1fr);
    gap: 5px 12px;
    margin: 0;
    align-content: start;
  }
  dt {
    color: var(--faint);
  }
  .facts dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .bad {
    color: var(--negative);
  }
  .forms {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  form label {
    font-size: 11.5px;
    color: var(--faint);
  }
  .field {
    display: flex;
    gap: 8px;
  }
  .field input {
    flex: 1;
  }
  .from {
    align-self: flex-start;
  }
  @media (max-width: 900px) {
    .speed,
    .info {
      grid-template-columns: minmax(0, 1fr);
    }
    .lane {
      grid-template-columns: 18px minmax(0, 1fr) 62px;
    }
    .range {
      display: none;
    }
  }
  @media (max-height: 620px) {
    .insp {
      height: 180px;
    }
  }
</style>
