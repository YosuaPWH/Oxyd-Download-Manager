<script lang="ts">
  import { api } from "../lib/api";
  import { canPause, canStart } from "../lib/actions";
  import * as fmt from "../lib/format";
  import { detailText, errorText, t, type Key } from "../lib/i18n.svelte";
  import { app, beginRefresh, clock, run, selected } from "../lib/store.svelte";
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

  // The last minute of speed for every running download, one sample per second.
  const history = new Map<string, number[]>();
  let samples = $state<number[]>([]);
  $effect(() => {
    void clock.now;
    for (const x of app.downloads) {
      if (x.status !== "downloading") {
        history.delete(x.id);
        continue;
      }
      const h = history.get(x.id) ?? [];
      h.push(x.speed);
      if (h.length > 61) h.shift();
      history.set(x.id, h);
    }
    samples = d ? [...(history.get(d.id) ?? [])] : [];
  });

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

  function host(url: string) {
    try {
      return new URL(url).host;
    } catch {
      return "";
    }
  }

  // Running downloads too: a dead link is usually still being retried.
  // Videos find their links again on the page by themselves.
  const canRefresh = $derived(!!d && !d.media && d.status !== "completed" && d.status !== "cancelled");
  const pct = $derived(d ? fmt.percent(d.downloaded, d.totalSize) : null);
  const moving = $derived(d?.status === "downloading");
  const runningSegs = $derived(d ? d.segments.filter((s) => s.downloaded < s.end - s.start + 1).length : 0);
</script>

<aside class="card" aria-label="Details">
  {#if d}
    <div>
      <h2>{d.filename || d.url}</h2>
      <div class="muted small">{t(`cat.${d.category}` as Key)} · {host(d.finalUrl ?? d.url)}</div>
    </div>

    <div>
      <div class="figure num">
        {fmt.bytes(d.downloaded)}{#if d.totalSize !== null}<span class="of">{` / ${fmt.bytes(d.totalSize)}`}</span>{/if}
      </div>
      <div class="muted line">
        {#if pct !== null}<span class="num ink">{pct.toFixed(1)}%</span>{:else}{t("detail.unknownSize")}{/if}
        {#if moving}
          · <span class="num ink">{fmt.speed(d.speed) || "0 B/s"}</span>
          {#if d.eta !== null}· <span class="num ink">{fmt.duration(d.eta)}</span> {t("detail.left")}{/if}
        {:else}
          · {t(`st.${d.status}` as Key)}
        {/if}
      </div>
      {#if d.detail && d.status !== "completed"}
        <div class="muted line">{detailText(d.detail)}</div>
      {/if}
    </div>

    <div class="buttons">
      {#if canPause(d)}
        <button onclick={() => d && run(() => api.pause(d.id))}>{t("act.pause")}</button>
      {:else if canStart(d)}
        <button onclick={() => d && run(() => api.start(d.id))}>{t("act.resume")}</button>
      {/if}
      {#if d.status === "completed"}
        <button onclick={() => d && run(() => api.openFile(d.id))}>{t("act.open")}</button>
      {/if}
      <button onclick={() => d && run(() => api.openFolder(d.id))}>{t("act.folder")}</button>
      <button onclick={() => d && run(() => api.copy(d.url))}>{t("act.copy")}</button>
    </div>

    {#if d.error}
      <div class="error" role="alert">
        <span class="pill bad">{t("st.failed")}</span>
        <span>{errorText(d.errorCode) ?? d.error}</span>
        {#if d.status === "failed" && d.retryAt}
          <span class="muted">{t("retry.in")} <span class="num ink">{fmt.duration(fmt.until(d.retryAt, clock.now)) || "0s"}</span></span>
        {/if}
      </div>
    {/if}

    {#if d.segments.length || d.status === "completed"}
      <div>
        <div class="caption">
          <span>{t("detail.segments")}</span>
          {#if d.status !== "completed"}
            <span><span class="num">{runningSegs}</span> {t("detail.of")} <span class="num">{d.segments.length}</span> {t("detail.running")}</span>
          {/if}
        </div>
        <SegmentBar segments={d.segments} total={d.totalSize} downloaded={d.downloaded} />
      </div>
    {/if}

    {#if moving}
      <div>
        <SpeedChart {samples} title={t("detail.speedMinute")} />
      </div>
    {/if}

    <dl>
      <dt>{t("detail.path")}</dt>
      <dd title={d.savePath}>{d.savePath || d.dir}</dd>
      <dt>{t("detail.url")}</dt>
      <dd class="ellipsis" title={d.finalUrl ?? d.url}>{d.url}</dd>
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
    </dl>

    {#if d.status !== "completed"}
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
            {t("refresh.fromBrowser")}
          </button>
        </form>
      {/if}
    {/if}
  {:else}
    <p class="muted none">{t("detail.none")}</p>
  {/if}
</aside>

<style>
  .card {
    align-self: start;
    min-height: 0;
    max-height: 100%;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 22px 24px 24px;
    background: var(--surface);
    border-radius: var(--radius);
    border: 1px solid var(--card-border);
    box-shadow: var(--card-shadow);
  }
  h2 {
    margin: 0;
    font-size: 16px;
    line-height: 22px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .small {
    font-size: 12px;
  }
  .figure {
    font-size: 30px;
    line-height: 36px;
    letter-spacing: -0.02em;
  }
  .of {
    font-size: 16px;
    letter-spacing: 0;
    color: var(--muted);
  }
  .line {
    margin-top: 4px;
  }
  .ink {
    color: var(--text);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .error {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }
  .caption {
    display: flex;
    justify-content: space-between;
    margin-bottom: 8px;
    font-size: 12px;
    color: var(--muted);
  }
  dl {
    display: grid;
    grid-template-columns: 96px minmax(0, 1fr);
    gap: 8px 12px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  form label {
    font-size: 12px;
    color: var(--muted);
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
  .none {
    margin: 0;
  }
</style>
