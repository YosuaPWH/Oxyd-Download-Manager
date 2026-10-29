<script lang="ts">
  import { api } from "../lib/api";
  import { askRemove, canPause, canStart, primaryAction } from "../lib/actions";
  import * as fmt from "../lib/format";
  import { detailText, errorText, t, type Key } from "../lib/i18n.svelte";
  import { app, beginRefresh, clock, matchesSearch, run, visible } from "../lib/store.svelte";
  import type { DownloadInfo } from "../lib/types";
  import Icon from "./Icon.svelte";

  const rows = $derived(
    app.downloads.filter(
      (d) =>
        visible(d, app.view) && (!app.category || d.category === app.category) && matchesSearch(d, app.search),
    ),
  );

  let menu = $state<{ x: number; y: number; d: DownloadInfo } | null>(null);
  let table: HTMLDivElement;

  function name(d: DownloadInfo) {
    return d.filename || d.url;
  }

  function host(d: DownloadInfo) {
    try {
      return new URL(d.finalUrl ?? d.url).host;
    } catch {
      return "";
    }
  }

  function select(d: DownloadInfo) {
    app.selected = d.id;
  }

  function keydown(e: KeyboardEvent) {
    if (!rows.length) return;
    const i = rows.findIndex((d) => d.id === app.selected);
    const cur = rows[i];
    const go = (j: number) => {
      const d = rows[Math.max(0, Math.min(rows.length - 1, j))];
      app.selected = d.id;
      table.querySelector<HTMLElement>(`[data-id="${d.id}"]`)?.scrollIntoView({ block: "nearest" });
      e.preventDefault();
    };
    switch (e.key) {
      case "ArrowDown":
        return go(i + 1);
      case "ArrowUp":
        return go(i < 0 ? 0 : i - 1);
      case "Home":
        return go(0);
      case "End":
        return go(rows.length - 1);
      case "Enter":
      case " ":
        if (cur) {
          e.preventDefault();
          primaryAction(cur);
        }
        return;
      case "Delete":
        if (cur) askRemove(cur);
        return;
      case "ContextMenu":
        if (cur) {
          const el = table.querySelector<HTMLElement>(`[data-id="${cur.id}"]`)!.getBoundingClientRect();
          openMenu(el.left + 40, el.bottom, cur);
          e.preventDefault();
        }
    }
  }

  function openMenu(x: number, y: number, d: DownloadInfo) {
    select(d);
    menu = { x: Math.min(x, innerWidth - 240), y: Math.min(y, innerHeight - 340), d };
  }

  function contextmenu(e: MouseEvent, d: DownloadInfo) {
    e.preventDefault();
    openMenu(e.clientX, e.clientY, d);
  }

  function kebab(e: MouseEvent, d: DownloadInfo) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openMenu(r.right - 230, r.bottom + 4, d);
  }

  /** Runs a menu action on the menu's download. The download is taken before the menu closes:
   *  `{@const d = menu.d}` is re-read on use and would be null afterwards. */
  function act(fn: (d: DownloadInfo) => Promise<unknown> | void) {
    if (!menu) return;
    const d = menu.d;
    menu = null;
    void run(async () => fn(d));
  }

  /** Progress fill colour: the accent only on what is moving. */
  function fill(d: DownloadInfo) {
    return d.status === "downloading" || d.status === "probing" ? "var(--accent)" : "var(--done)";
  }

  function status(d: DownloadInfo) {
    const doing = d.detail && d.detail !== "ytdlp" ? detailText(d.detail) : null;
    if (doing && (d.status === "probing" || d.status === "downloading")) return doing;
    if (d.status === "downloading" && d.activeConnections > 0) return `${t("st.downloading")} · ${d.activeConnections}×`;
    if (d.status === "failed" && d.retryAt) return `${t("st.failed")} · ↻ ${fmt.duration(fmt.until(d.retryAt, clock.now)) || "0s"}`;
    return t(`st.${d.status}` as Key);
  }
</script>

<svelte:window onclick={() => (menu = null)} onblur={() => (menu = null)} />

<div class="wrap">
  <div
    class="table"
    role="grid"
    tabindex="0"
    aria-label={t("page.downloads")}
    aria-rowcount={rows.length}
    bind:this={table}
    onkeydown={keydown}
  >
    <div class="row head" role="row">
      <div role="columnheader">{t("col.name")}</div>
      <div role="columnheader" class="r">{t("col.size")}</div>
      <div role="columnheader" class="r">{t("col.progress")}</div>
      <div role="columnheader" class="r">{t("col.speed")}</div>
      <div role="columnheader" class="r">{t("col.eta")}</div>
      <div role="columnheader" class="st">{t("col.status")}</div>
      <div role="columnheader"><span class="sr-only">{t("act.more")}</span></div>
    </div>
    {#each rows as d (d.id)}
      {@const pct = fmt.percent(d.downloaded, d.totalSize)}
      {@const moving = d.status === "downloading"}
      <!-- Keyboard navigation is handled by the grid (arrow keys, Enter, Delete). -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row"
        role="row"
        tabindex="-1"
        aria-selected={app.selected === d.id}
        data-id={d.id}
        onclick={() => select(d)}
        ondblclick={() => primaryAction(d)}
        oncontextmenu={(e) => contextmenu(e, d)}
      >
        <div class="name" role="gridcell" title={d.url}>
          <span class="ellipsis file">{name(d)}</span>
          <span class="ellipsis muted sub">{t(`cat.${d.category}` as Key)}{host(d) ? ` · ${host(d)}` : ""}</span>
        </div>
        <div role="gridcell" class="r num">{d.totalSize === null ? "—" : fmt.bytes(d.totalSize)}</div>
        <div role="gridcell" class="prog">
          <div class="track" class:indet={pct === null && moving}>
            <div class="fill" style:width="{pct ?? 0}%" style:background={fill(d)}></div>
          </div>
          <span class="num pct">{pct === null ? fmt.bytes(d.downloaded) : `${pct.toFixed(1)}%`}</span>
        </div>
        <div role="gridcell" class="r num" class:muted={!moving}>{moving ? fmt.speed(d.speed) || "0 B/s" : "—"}</div>
        <div role="gridcell" class="r num" class:muted={!moving || d.eta === null}>{moving && d.eta !== null ? fmt.duration(d.eta) : "—"}</div>
        <div role="gridcell" class="st" title={errorText(d.errorCode) ?? d.error ?? ""}>
          {#if d.status === "failed"}
            <span class="pill bad">{status(d)}</span>
          {:else}
            <span class="ellipsis" class:muted={!moving && d.status !== "probing"}>{status(d)}</span>
          {/if}
        </div>
        <div role="gridcell" class="r">
          <button class="icon" aria-label={t("act.more")} tabindex="-1" onclick={(e) => kebab(e, d)}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="12" cy="5" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="12" cy="19" r="1.6" /></svg>
          </button>
        </div>
      </div>
    {:else}
      <div class="empty">
        <h2>{t("empty.title")}</h2>
        <p class="muted">{t("empty.body")}</p>
        <button class="primary" onclick={() => (app.manualUrl = "")}><Icon name="add" size={14} />{t("tb.add")}</button>
      </div>
    {/each}
  </div>
</div>

{#if menu}
  {@const d = menu.d}
  <div class="menu" role="menu" tabindex="-1" style:left="{menu.x}px" style:top="{menu.y}px" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (menu = null)}>
    {#if d.status === "completed"}
      <button role="menuitem" onclick={() => act((d) => api.openFile(d.id))}><Icon name="file" />{t("act.open")}</button>
    {/if}
    <button role="menuitem" onclick={() => act((d) => api.openFolder(d.id))}><Icon name="folder" />{t("act.folder")}</button>
    <button role="menuitem" onclick={() => act((d) => api.copy(d.url))}><Icon name="copy" />{t("act.copy")}</button>
    <hr />
    {#if canStart(d)}
      <button role="menuitem" onclick={() => act((d) => api.start(d.id))}><Icon name="play" />{t("act.resume")}</button>
    {/if}
    {#if canPause(d)}
      <button role="menuitem" onclick={() => act((d) => api.pause(d.id))}><Icon name="pause" />{t("act.pause")}</button>
    {/if}
    {#if d.status !== "completed" && d.status !== "cancelled"}
      <button role="menuitem" onclick={() => act((d) => api.cancel(d.id))}><Icon name="stop" />{t("act.cancel")}</button>
    {/if}
    {#if d.status !== "completed" && d.status !== "cancelled"}
      <button role="menuitem" onclick={() => act((d) => beginRefresh(d))}><Icon name="link" />{t("refresh.fromBrowser")}</button>
    {/if}
    {#if d.status === "failed" || d.status === "paused" || d.status === "completed"}
      <button role="menuitem" onclick={() => act((d) => api.restart(d.id))}><Icon name="refresh" />{t("act.restart")}</button>
    {/if}
    <hr />
    <button role="menuitem" onclick={() => act((d) => api.move(d.id, -1))}><Icon name="up" />{t("act.up")}</button>
    <button role="menuitem" onclick={() => act((d) => api.move(d.id, 1))}><Icon name="down" />{t("act.down")}</button>
    <hr />
    <button role="menuitem" class="danger" onclick={() => act((d) => askRemove(d))}><Icon name="trash" />{t("act.remove")}</button>
  </div>
{/if}

<style>
  .wrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .table {
    outline: none;
    min-width: 720px;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) 84px minmax(140px, 176px) 92px 72px minmax(110px, 150px) 32px;
    column-gap: 16px;
    align-items: center;
    height: 56px;
    padding: 0 6px 0 10px;
    border-top: 1px solid var(--rule);
    cursor: default;
    user-select: none;
  }
  .row:not(.head):hover {
    background: var(--raised);
  }
  .row[aria-selected="true"] {
    background: var(--raised);
  }
  .table:focus-visible .row[aria-selected="true"] {
    box-shadow: inset 1px 0 0 var(--accent);
  }
  .head {
    position: sticky;
    top: 0;
    z-index: 1;
    height: 38px;
    border-top: 0;
    background: var(--ground);
    font-size: 12px;
    color: var(--muted);
  }
  .r {
    text-align: right;
  }
  .st {
    min-width: 0;
    display: flex;
  }
  .name {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .file {
    font-weight: 500;
  }
  .sub {
    font-size: 12px;
    line-height: 18px;
  }
  .prog {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .track {
    flex: 1;
    height: 2px;
    background: var(--track);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    transition: width 0.25s linear;
  }
  .track.indet .fill {
    width: 30% !important;
    animation: slide 1.2s linear infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .pct {
    width: 52px;
    text-align: right;
  }
  .row .icon {
    width: 28px;
    height: 28px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 32px 10px;
    border-top: 1px solid var(--rule);
  }
  .empty h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .empty p {
    margin: 0 0 12px;
  }
  .menu {
    position: fixed;
    z-index: 40;
    min-width: 230px;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--float-shadow);
  }
  .menu button {
    width: 100%;
    height: 32px;
    border: none;
    border-radius: 4px;
    justify-content: flex-start;
    padding: 0 10px;
    gap: 10px;
  }
  .menu hr {
    border: none;
    border-top: 1px solid var(--rule);
    margin: 4px 0;
  }
</style>
