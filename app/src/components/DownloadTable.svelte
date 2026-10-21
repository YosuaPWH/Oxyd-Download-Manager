<script lang="ts">
  import { api } from "../lib/api";
  import { askRemove, canPause, canStart, primaryAction } from "../lib/actions";
  import * as fmt from "../lib/format";
  import { errorText, t, type Key } from "../lib/i18n.svelte";
  import { app, beginRefresh, clock, matchesSearch, run, visible } from "../lib/store.svelte";
  import type { DownloadInfo } from "../lib/types";
  import Icon from "./Icon.svelte";

  const rows = $derived(
    app.downloads.filter((d) => visible(d, app.view) && matchesSearch(d, app.search)),
  );

  let menu = $state<{ x: number; y: number; d: DownloadInfo } | null>(null);
  let table: HTMLDivElement;

  function name(d: DownloadInfo) {
    return d.filename || d.url;
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
          menu = { x: el.left + 40, y: el.bottom, d: cur };
          e.preventDefault();
        }
    }
  }

  function contextmenu(e: MouseEvent, d: DownloadInfo) {
    e.preventDefault();
    select(d);
    menu = { x: Math.min(e.clientX, innerWidth - 230), y: Math.min(e.clientY, innerHeight - 330), d };
  }

  function act(fn: () => Promise<unknown> | void) {
    menu = null;
    void run(async () => fn());
  }

  const statusClass: Record<string, string> = {
    downloading: "run",
    probing: "run",
    queued: "wait",
    paused: "wait",
    completed: "ok",
    failed: "bad",
    cancelled: "muted",
  };
</script>

<svelte:window onclick={() => (menu = null)} onblur={() => (menu = null)} />

<div class="wrap">
  <div
    class="table"
    role="grid"
    tabindex="0"
    aria-label="Downloads"
    aria-rowcount={rows.length}
    bind:this={table}
    onkeydown={keydown}
  >
    <div class="row head" role="row">
      <div role="columnheader">{t("col.name")}</div>
      <div role="columnheader" class="num">{t("col.size")}</div>
      <div role="columnheader">{t("col.progress")}</div>
      <div role="columnheader" class="num">{t("col.speed")}</div>
      <div role="columnheader" class="num">{t("col.eta")}</div>
      <div role="columnheader">{t("col.status")}</div>
    </div>
    {#each rows as d (d.id)}
      {@const pct = fmt.percent(d.downloaded, d.totalSize)}
      <!-- Keyboard navigation is handled by the grid (arrow keys, Enter, Delete). -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row"
        role="row"
        tabindex="-1"
        aria-selected={app.selected === d.id}
        class:sel={app.selected === d.id}
        data-id={d.id}
        onclick={() => select(d)}
        ondblclick={() => primaryAction(d)}
        oncontextmenu={(e) => contextmenu(e, d)}
      >
        <div class="name" role="gridcell" title={d.url}>
          <span class="ico"><Icon name={d.category} /></span>
          <span class="ellipsis">{name(d)}</span>
        </div>
        <div role="gridcell" class="num">{fmt.bytes(d.totalSize)}</div>
        <div role="gridcell" class="prog">
          <div class="pbar" class:indet={pct === null && d.status === "downloading"}>
            <div class="pfill {statusClass[d.status]}" style:width="{pct ?? 0}%"></div>
          </div>
          <span class="pct">{pct === null ? fmt.bytes(d.downloaded) : `${pct.toFixed(1)}%`}</span>
        </div>
        <div role="gridcell" class="num">{d.status === "downloading" ? fmt.speed(d.speed) : ""}</div>
        <div role="gridcell" class="num">{d.status === "downloading" ? fmt.duration(d.eta) : ""}</div>
        <div role="gridcell" class="st {statusClass[d.status]}" title={errorText(d.errorCode) ?? d.error ?? ""}>
          {t(`st.${d.status}` as Key)}
          {#if d.status === "failed" && d.retryAt}
            <span class="muted">· ↻ {fmt.duration(fmt.until(d.retryAt, clock.now))}</span>
          {/if}
          {#if d.status === "downloading" && d.activeConnections > 0}
            <span class="muted">· {d.activeConnections}×</span>
          {/if}
        </div>
      </div>
    {:else}
      <div class="empty">
        <Icon name="active" size={40} />
        <h3>{t("empty.title")}</h3>
        <p class="muted">{t("empty.body")}</p>
      </div>
    {/each}
  </div>
</div>

{#if menu}
  {@const d = menu.d}
  <div class="menu" role="menu" tabindex="-1" style:left="{menu.x}px" style:top="{menu.y}px" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (menu = null)}>
    {#if d.status === "completed"}
      <button role="menuitem" onclick={() => act(() => api.openFile(d.id))}><Icon name="file" />{t("act.open")}</button>
    {/if}
    <button role="menuitem" onclick={() => act(() => api.openFolder(d.id))}><Icon name="folder" />{t("act.folder")}</button>
    <button role="menuitem" onclick={() => act(() => api.copy(d.url))}><Icon name="copy" />{t("act.copy")}</button>
    <hr />
    {#if canStart(d)}
      <button role="menuitem" onclick={() => act(() => api.start(d.id))}><Icon name="play" />{t("act.resume")}</button>
    {/if}
    {#if canPause(d)}
      <button role="menuitem" onclick={() => act(() => api.pause(d.id))}><Icon name="pause" />{t("act.pause")}</button>
    {/if}
    {#if d.status !== "completed" && d.status !== "cancelled"}
      <button role="menuitem" onclick={() => act(() => api.cancel(d.id))}><Icon name="stop" />{t("act.cancel")}</button>
    {/if}
    {#if d.status === "failed" || d.status === "paused"}
      <button role="menuitem" onclick={() => act(() => beginRefresh(d))}><Icon name="link" />{t("refresh.fromBrowser")}</button>
    {/if}
    {#if d.status === "failed" || d.status === "paused" || d.status === "completed"}
      <button role="menuitem" onclick={() => act(() => api.restart(d.id))}><Icon name="refresh" />{t("act.restart")}</button>
    {/if}
    <hr />
    <button role="menuitem" onclick={() => act(() => api.move(d.id, -1))}><Icon name="up" />{t("act.up")}</button>
    <button role="menuitem" onclick={() => act(() => api.move(d.id, 1))}><Icon name="down" />{t("act.down")}</button>
    <hr />
    <button role="menuitem" class="danger" onclick={() => act(() => askRemove(d))}><Icon name="trash" />{t("act.remove")}</button>
  </div>
{/if}

<style>
  .wrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: var(--panel);
  }
  .table {
    outline: none;
    min-width: 640px;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(200px, 1fr) 90px minmax(150px, 220px) 100px 90px 130px;
    gap: 12px;
    align-items: center;
    padding: 0 14px;
    height: 34px;
    border-bottom: 1px solid var(--border);
    cursor: default;
    user-select: none;
  }
  .row:not(.head):hover {
    background: var(--row-hover);
  }
  .row.sel {
    background: var(--row-sel);
  }
  .table:focus-visible .row.sel {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .head {
    position: sticky;
    top: 0;
    z-index: 1;
    height: 30px;
    background: var(--panel-2);
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .name {
    display: flex;
    gap: 8px;
    align-items: center;
    min-width: 0;
  }
  .ico {
    color: var(--muted);
    display: flex;
  }
  .prog {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .pbar {
    flex: 1;
    height: 8px;
    border-radius: 4px;
    background: var(--panel-2);
    overflow: hidden;
  }
  .pfill {
    height: 100%;
    background: var(--muted);
    transition: width 0.25s linear;
  }
  .pfill.run {
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
  }
  .pfill.ok {
    background: var(--ok);
  }
  .pfill.bad {
    background: var(--danger);
  }
  .pbar.indet .pfill {
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
    width: 56px;
    text-align: right;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .st.run {
    color: var(--accent);
  }
  .st.ok {
    color: var(--ok);
  }
  .st.bad {
    color: var(--danger);
  }
  .st.wait {
    color: var(--warn);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 80px 20px;
    color: var(--muted);
  }
  .empty h3 {
    margin: 12px 0 4px;
    color: var(--text);
  }
  .menu {
    position: fixed;
    z-index: 40;
    min-width: 220px;
    padding: 4px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }
  .menu button {
    width: 100%;
    border: none;
    background: transparent;
    justify-content: flex-start;
    padding: 6px 10px;
    gap: 10px;
  }
  .menu hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 4px 0;
  }
</style>
