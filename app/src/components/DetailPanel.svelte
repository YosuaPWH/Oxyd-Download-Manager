<script lang="ts">
  import { api } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { run, selected } from "../lib/store.svelte";
  import SegmentBar from "./SegmentBar.svelte";

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

  const canRefresh = $derived(!!d && (d.status === "paused" || d.status === "failed"));
</script>

{#if d}
  <section class="detail" aria-label="Details">
    <div class="segments">
      <div class="label muted">
        {t("detail.segments")} · {d.segments.length}
        {#if d.totalSize}· {fmt.bytes(d.downloaded)} / {fmt.bytes(d.totalSize)}{/if}
      </div>
      <SegmentBar segments={d.segments} total={d.totalSize} downloaded={d.downloaded} />
    </div>
    <dl>
      <dt>{t("detail.url")}</dt>
      <dd class="ellipsis mono" title={d.finalUrl ?? d.url}>{d.url}</dd>
      <dt>{t("detail.path")}</dt>
      <dd class="ellipsis" title={d.savePath}>{d.savePath || d.dir}</dd>
      <dt>{t("detail.resumable")}</dt>
      <dd>{d.resumable ? t("yes") : t("no")} · {t("detail.connections")}: {d.connections}</dd>
      <dt>{t("detail.added")}</dt>
      <dd>
        {fmt.date(d.createdAt)}
        {#if d.completedAt}· {t("detail.completedAt")}: {fmt.date(d.completedAt)}{/if}
      </dd>
      {#if d.error}
        <dt>{t("detail.error")}</dt>
        <dd class="error">{d.error}</dd>
      {/if}
    </dl>
    {#if d.status !== "completed"}
      <div class="forms">
        <form onsubmit={(e) => (e.preventDefault(), applyLimit())}>
          <label for="limit">{t("detail.limit")}</label>
          <input id="limit" bind:value={limit} placeholder={t("detail.limitHint")} />
          <button type="submit">{t("detail.apply")}</button>
        </form>
        {#if canRefresh}
          <form onsubmit={(e) => (e.preventDefault(), applyUrl())}>
            <label for="newurl">{t("act.refresh")}</label>
            <input id="newurl" bind:value={newUrl} placeholder={t("detail.newUrl")} />
            <button type="submit" disabled={!newUrl.trim()}>{t("detail.apply")}</button>
          </form>
        {/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .detail {
    flex-shrink: 0;
    border-top: 1px solid var(--border);
    background: var(--panel);
    padding: 10px 16px 12px;
    display: grid;
    gap: 8px;
  }
  .label {
    font-size: 12px;
    margin-bottom: 4px;
  }
  dl {
    display: grid;
    grid-template-columns: 120px 1fr;
    gap: 3px 12px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  .error {
    color: var(--danger);
  }
  .forms {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
  }
  form {
    display: flex;
    gap: 8px;
    align-items: center;
    flex: 1;
    min-width: 280px;
  }
  form label {
    color: var(--muted);
    white-space: nowrap;
  }
  form input {
    flex: 1;
  }
</style>
