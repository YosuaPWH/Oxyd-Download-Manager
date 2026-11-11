<script lang="ts">
  import { tick } from "svelte";
  import { api } from "../lib/api";
  import { askRemove, canPause, canStart, hostOf, pauseSelected, primaryAction, startSelected } from "../lib/actions";
  import * as fmt from "../lib/format";
  import { detailText, errorText, i18n, t, type Key } from "../lib/i18n.svelte";
  import {
    app,
    beginRefresh,
    clock,
    listed,
    run,
    saveLayout,
    selectedDownloads,
    selectOnly,
    type SortKey,
  } from "../lib/store.svelte";
  import type { DownloadInfo } from "../lib/types";
  import FileTile from "./FileTile.svelte";
  import Icon from "./Icon.svelte";

  const rows = $derived(listed());
  const picked = $derived(new Set(app.selection));
  const allPicked = $derived(rows.length > 0 && rows.every((d) => picked.has(d.id)));

  // Windowing: only the rows near the viewport are in the DOM. Without this
  // a long finished history (hundreds of rows) is rendered, laid out and
  // kept alive on every 250 ms progress tick, even though most of it is
  // never on screen. Row and header heights must match the CSS (--row-h).
  const ROW_H = $derived(app.settings?.density === "compact" ? 32 : 44);
  const HEAD_H = 32;
  const OVERSCAN = 10;
  let menu = $state<{ x: number; y: number; d: DownloadInfo } | null>(null);
  let table: HTMLDivElement;
  let wrap: HTMLDivElement;
  let scrollTop = $state(0);
  let viewH = $state(600);

  const totalH = $derived(HEAD_H + rows.length * ROW_H);
  const clampedTop = $derived(Math.max(0, Math.min(scrollTop, Math.max(0, totalH - viewH))));
  const start = $derived(Math.max(0, Math.floor((clampedTop - HEAD_H) / ROW_H) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((clampedTop + viewH - HEAD_H) / ROW_H) + OVERSCAN));
  const visibleRows = $derived(rows.slice(start, end));

  function onScroll() {
    if (!wrap) return;
    scrollTop = wrap.scrollTop;
    viewH = wrap.clientHeight;
  }

  /** Scroll so row `idx` is visible, for rows outside the rendered window. */
  async function ensureVisible(idx: number, down: boolean) {
    if (!wrap) return;
    const top = HEAD_H + idx * ROW_H;
    if (down) wrap.scrollTop = Math.max(0, top + ROW_H - viewH);
    else wrap.scrollTop = Math.max(0, top - HEAD_H);
    await tick();
  }

  function name(d: DownloadInfo) {
    return d.filename || d.url;
  }

  function toggle(d: DownloadInfo) {
    if (picked.has(d.id)) {
      app.selection = app.selection.filter((id) => id !== d.id);
      if (app.selected === d.id) app.selected = app.selection[app.selection.length - 1] ?? null;
    } else {
      app.selection = [...app.selection, d.id];
      app.selected = d.id;
    }
  }

  /** Select from the anchor (the focused row) through `d`. */
  function selectRange(d: DownloadInfo) {
    const a = rows.findIndex((x) => x.id === app.selected);
    const b = rows.findIndex((x) => x.id === d.id);
    if (a < 0 || b < 0) return selectOnly(d.id);
    const [lo, hi] = a < b ? [a, b] : [b, a];
    app.selection = rows.slice(lo, hi + 1).map((x) => x.id);
  }

  function click(e: MouseEvent, d: DownloadInfo) {
    if (e.shiftKey) selectRange(d);
    else if (e.ctrlKey || e.metaKey) toggle(d);
    else selectOnly(d.id);
  }

  function toggleAll() {
    if (allPicked) selectOnly(null);
    else {
      app.selection = rows.map((d) => d.id);
      app.selected ??= rows[0]?.id ?? null;
    }
  }

  function keydown(e: KeyboardEvent) {
    if (!rows.length) return;
    const i = rows.findIndex((d) => d.id === app.selected);
    const cur = rows[i];
    const go = async (j: number) => {
      // Prevent the browser's own scroll synchronously: the awaits below
      // must not let it run first, or the list would jump twice.
      e.preventDefault();
      const idx = Math.max(0, Math.min(rows.length - 1, j));
      const d = rows[idx];
      if (e.shiftKey && app.selected) {
        // Extend: the anchor stays where the range started.
        if (!picked.has(d.id)) app.selection = [...app.selection, d.id];
        app.selected = d.id;
      } else selectOnly(d.id);
      await tick();
      const el = table.querySelector<HTMLElement>(`[data-id="${d.id}"]`);
      if (el) el.scrollIntoView({ block: "nearest" });
      else await ensureVisible(idx, idx > i);
    };
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      app.selection = rows.map((d) => d.id);
      app.selected ??= rows[0].id;
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        return go(i + 1);
      case "ArrowUp":
        return go(i < 0 ? 0 : i - 1);
      case "Home":
        return go(0);
      case "End":
        return go(rows.length - 1);
      case "Escape":
        if (app.selection.length > 1) selectOnly(app.selected);
        return;
      case "Enter":
      case " ":
        if (cur) {
          e.preventDefault();
          primaryAction(cur);
        }
        return;
      case "Delete":
        askRemove(selectedDownloads());
        return;
      case "ContextMenu":
        if (cur) {
          e.preventDefault();
          void (async () => {
            let el = table.querySelector<HTMLElement>(`[data-id="${cur.id}"]`);
            if (!el) {
              // Selected row is outside the rendered window: bring it in
              // first instead of throwing on a missing element.
              await ensureVisible(i, false);
              el = table.querySelector<HTMLElement>(`[data-id="${cur.id}"]`);
            }
            if (el) {
              const r = el.getBoundingClientRect();
              openMenu(r.left + 80, r.bottom, cur);
            }
          })();
        }
    }
  }

  function openMenu(x: number, y: number, d: DownloadInfo) {
    // A right click inside the selection acts on all of it; outside, on that row alone.
    if (!picked.has(d.id)) selectOnly(d.id);
    else app.selected = d.id;
    menu = { x: Math.max(8, Math.min(x, innerWidth - 240)), y: Math.max(8, Math.min(y, innerHeight - 350)), d };
  }

  function contextmenu(e: MouseEvent, d: DownloadInfo) {
    e.preventDefault();
    openMenu(e.clientX, e.clientY, d);
  }

  function kebab(e: MouseEvent, d: DownloadInfo) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    if (!picked.has(d.id) || app.selection.length > 1) selectOnly(d.id);
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

  function status(d: DownloadInfo) {
    const doing = d.detail && d.detail !== "ytdlp" ? detailText(d.detail) : null;
    if (doing && (d.status === "probing" || d.status === "downloading")) return doing;
    if (d.status === "downloading" && d.activeConnections > 0) return `${t("st.downloading")} · ${d.activeConnections}×`;
    if (d.status === "failed" && d.retryAt) return `${t("st.failed")} · ↻ ${fmt.duration(fmt.until(d.retryAt, clock.now)) || "0s"}`;
    return t(`st.${d.status}` as Key);
  }

  const STATUS_ICON: Record<DownloadInfo["status"], string> = {
    queued: "clock",
    probing: "refresh",
    downloading: "active",
    paused: "pause",
    completed: "completed",
    failed: "alert",
    cancelled: "x",
  };

  /** "Today 10:12", "Yesterday", or the date. */
  function added(unix: number) {
    const d = new Date(unix * 1000);
    const today = new Date(clock.now * 1000);
    const days = Math.round((new Date(today.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 86400000);
    if (days === 0) return d.toLocaleTimeString(i18n.lang, { hour: "2-digit", minute: "2-digit" });
    if (days === 1) return t("date.yesterday");
    return d.toLocaleDateString(i18n.lang, { day: "numeric", month: "short", year: days > 300 ? "numeric" : undefined });
  }

  // Header: click sorts ascending, again descending, a third time returns to queue order.
  const columns: { key: SortKey; label: Key; cls: string }[] = [
    { key: "name", label: "col.name", cls: "" },
    { key: "size", label: "col.size", cls: "r" },
    { key: "status", label: "col.status", cls: "" },
    { key: "progress", label: "col.progress", cls: "" },
    { key: "speed", label: "col.speed", cls: "r" },
    { key: "eta", label: "col.eta", cls: "r c-eta" },
    { key: "added", label: "col.added", cls: "r c-added" },
  ];

  function sortBy(key: SortKey) {
    if (app.sort.key !== key) app.sort = { key, dir: key === "name" ? 1 : -1 };
    else if (app.sort.dir === (key === "name" ? 1 : -1)) app.sort = { key, dir: key === "name" ? -1 : 1 };
    else app.sort = { key: "queue", dir: 1 };
    saveLayout();
  }

  const many = $derived(app.selection.length > 1);
</script>

<svelte:window onclick={() => (menu = null)} onblur={() => (menu = null)} />

<div class="wrap" bind:this={wrap} bind:clientHeight={viewH} onscroll={onScroll}>
  <div
    class="table"
    role="grid"
    tabindex="0"
    aria-label={t("page.downloads")}
    aria-rowcount={rows.length}
    aria-multiselectable="true"
    bind:this={table}
    onkeydown={keydown}
  >
    <div class="row head" role="row">
      <div role="columnheader" class="pick">
        <input type="checkbox" aria-label={t("list.selectAll")} checked={allPicked} onchange={toggleAll} disabled={!rows.length} />
      </div>
      <div role="columnheader" class="c-tile"></div>
      {#each columns as c (c.key)}
        <div
          role="columnheader"
          class={c.cls}
          aria-sort={app.sort.key === c.key ? (app.sort.dir === 1 ? "ascending" : "descending") : "none"}
        >
          <button class="sort" onclick={() => sortBy(c.key)} tabindex="-1">
            {t(c.label)}
            {#if app.sort.key === c.key}<Icon name={app.sort.dir === 1 ? "chevronUp" : "chevronDown"} size={12} />{/if}
          </button>
        </div>
      {/each}
      <div role="columnheader"><span class="sr-only">{t("act.more")}</span></div>
    </div>
    {#if rows.length}
      <!-- Spacers keep the scrollbar and total height identical to the full list. -->
      <div style:height="{start * ROW_H}px" aria-hidden="true"></div>
    {/if}
    {#each visibleRows as d (d.id)}
      {@const pct = fmt.percent(d.downloaded, d.totalSize)}
      {@const moving = d.status === "downloading"}
      {@const host = hostOf(d)}
      <!-- Keyboard navigation is handled by the grid (arrow keys, Enter, Delete). -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row {d.status}"
        class:focus={app.selected === d.id}
        role="row"
        tabindex="-1"
        aria-selected={picked.has(d.id)}
        data-id={d.id}
        onclick={(e) => click(e, d)}
        ondblclick={() => primaryAction(d)}
        oncontextmenu={(e) => contextmenu(e, d)}
      >
        <div role="gridcell" class="pick">
          <input
            type="checkbox"
            tabindex="-1"
            aria-label={name(d)}
            checked={picked.has(d.id)}
            onclick={(e) => e.stopPropagation()}
            onchange={() => toggle(d)}
          />
        </div>
        <div role="gridcell" class="c-tile"><FileTile category={d.category} /></div>
        <div class="name" role="gridcell" title={d.url}>
          <span class="ellipsis file">{name(d)}</span>
          <span class="ellipsis faint sub">{host ? `${host} · ` : ""}{t(`cat.${d.category}` as Key)}{d.media?.height ? ` · ${d.media.height}p` : ""}</span>
        </div>
        <div role="gridcell" class="r num">{d.totalSize === null ? "—" : fmt.bytes(d.totalSize)}</div>
        <div role="gridcell" class="st" title={errorText(d.errorCode) ?? d.error ?? ""}>
          <Icon name={STATUS_ICON[d.status]} size={13} /><span class="ellipsis">{status(d)}</span>
        </div>
        <div role="gridcell" class="prog">
          <div class="figures num">
            <b>{pct === null ? fmt.bytes(d.downloaded) : `${pct.toFixed(1)}%`}</b>
            {#if pct !== null && d.status !== "completed"}<span class="faint ellipsis">{fmt.bytes(d.downloaded)} / {fmt.bytes(d.totalSize)}</span>{/if}
          </div>
          <div class="track" class:indet={pct === null && moving}>
            <div class="fill" style:width="{pct ?? 0}%"></div>
          </div>
        </div>
        <div role="gridcell" class="r num" class:faint={!moving}>{moving ? fmt.speed(d.speed) || "0 B/s" : "—"}</div>
        <div role="gridcell" class="r num c-eta" class:faint={!moving || d.eta === null}>{moving && d.eta !== null ? fmt.duration(d.eta) : "—"}</div>
        <div role="gridcell" class="r num faint c-added" title={fmt.date(d.createdAt)}>{added(d.createdAt)}</div>
        <div role="gridcell" class="r">
          <button class="icon" aria-label={t("act.more")} tabindex="-1" onclick={(e) => kebab(e, d)}>
            <Icon name="more" size={16} />
          </button>
        </div>
      </div>
    {/each}
    {#if rows.length}
      <div style:height="{(rows.length - end) * ROW_H}px" aria-hidden="true"></div>
    {:else}
      <div class="empty">
        <svg width="56" height="56" viewBox="0 0 28 28" aria-hidden="true">
          <circle cx="14" cy="14" r="11" />
          <path d="M14 3A11 11 0 0 1 21.78 6.22" />
        </svg>
        {#if app.downloads.length}
          <h2>{t("empty.filtered")}</h2>
          <p class="muted">{t("empty.filteredBody")}</p>
        {:else}
          <h2>{t("empty.title")}</h2>
          <p class="muted">{t("empty.body")}</p>
          <div class="cta">
            <button class="primary" onclick={() => (app.manualUrl = "")}><Icon name="add" size={14} />{t("tb.add")}</button>
            <button onclick={() => (app.batch = "")}><Icon name="layers" size={14} />{t("tb.batch")}</button>
          </div>
          <p class="faint tip">{t("empty.tip")}</p>
        {/if}
      </div>
    {/if}
  </div>
</div>

{#if menu}
  {@const d = menu.d}
  <div class="menu" role="menu" tabindex="-1" style:left="{menu.x}px" style:top="{menu.y}px" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (menu = null)}>
    {#if many}
      <div class="menuhead faint"><span class="num">{app.selection.length}</span> {t("sel.count")}</div>
      <button role="menuitem" onclick={() => ((menu = null), void startSelected())}><Icon name="play" />{t("act.resume")}</button>
      <button role="menuitem" onclick={() => ((menu = null), void pauseSelected())}><Icon name="pause" />{t("act.pause")}</button>
      <hr />
      <button role="menuitem" class="danger" onclick={() => ((menu = null), askRemove(selectedDownloads()))}><Icon name="trash" />{t("act.remove")}</button>
    {:else}
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
    {/if}
  </div>
{/if}

<style>
  .wrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
    container-type: inline-size;
  }
  .table {
    outline: none;
    min-width: 600px;
  }
  .row {
    display: grid;
    grid-template-columns: 20px 30px minmax(120px, 1fr) 72px 128px minmax(120px, 168px) 82px 64px 84px 28px;
    column-gap: 12px;
    align-items: center;
    height: var(--row-h);
    padding: 0 8px 0 14px;
    border-bottom: 1px solid var(--rule);
    cursor: default;
    user-select: none;
    --state: var(--faint);
  }
  .row:not(.head):hover {
    background: var(--hover);
  }
  .row[aria-selected="true"] {
    background: var(--accent-softer);
  }
  .row[aria-selected="true"]:hover {
    background: var(--accent-soft);
  }
  .row.focus[aria-selected="true"] {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  /* Colour only on what is moving or needs attention. */
  .row.downloading {
    --state: var(--accent);
  }
  .row.probing {
    --state: var(--info);
  }
  .row.paused {
    --state: var(--warning);
  }
  .row.failed {
    --state: var(--negative);
  }
  .row.completed {
    --state: var(--positive);
  }
  .head {
    position: sticky;
    top: 0;
    z-index: 1;
    height: 32px;
    background: var(--surface);
    font-size: 11.5px;
    color: var(--faint);
  }
  .sort {
    height: 24px;
    padding: 0;
    gap: 3px;
    border: 0;
    font-size: 11.5px;
    font-weight: 500;
    color: inherit;
  }
  .sort:hover:not(:disabled) {
    background: none;
    color: var(--text);
  }
  [aria-sort="ascending"] .sort,
  [aria-sort="descending"] .sort {
    color: var(--text);
  }
  .pick {
    display: grid;
    place-items: center;
  }
  .pick input {
    width: 14px;
    height: 14px;
    margin: 0;
  }
  /* The checkbox appears when it is useful: on hover, and while anything is selected. */
  .row:not(.head) .pick input {
    opacity: 0;
  }
  .row:not(.head):hover .pick input,
  .row[aria-selected="true"] .pick input {
    opacity: 1;
  }
  .r {
    text-align: right;
    justify-content: flex-end;
  }
  .head .r {
    display: flex;
  }
  .st {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--state);
  }
  .row.completed .st,
  .row.queued .st,
  .row.cancelled .st {
    color: var(--muted);
  }
  .row.completed .st :global(svg) {
    color: var(--positive);
  }
  .st :global(svg) {
    flex: none;
  }
  .name {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .file {
    font-weight: 500;
    line-height: 18px;
  }
  .sub {
    font-size: 11.5px;
    line-height: 16px;
  }
  .prog {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  .figures {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 11.5px;
    line-height: 14px;
  }
  .figures b {
    font-weight: 500;
  }
  .track {
    height: 3px;
    border-radius: 2px;
    background: var(--track);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 2px;
    background: var(--state);
    transition: width 0.25s linear;
  }
  .row.completed .fill {
    background: var(--done);
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
  .row .icon {
    width: 26px;
    height: 26px;
  }
  /* Compact rows: one line, the figures beside the bar. */
  :global(:root[data-density="compact"]) .sub {
    display: none;
  }
  :global(:root[data-density="compact"]) .row {
    grid-template-columns: 20px 22px minmax(120px, 1fr) 72px 128px minmax(120px, 168px) 82px 64px 84px 28px;
  }
  :global(:root[data-density="compact"]) .prog {
    flex-direction: row-reverse;
    align-items: center;
    gap: 8px;
  }
  :global(:root[data-density="compact"]) .track {
    flex: 1;
  }
  :global(:root[data-density="compact"]) .figures span {
    display: none;
  }
  :global(:root[data-density="compact"]) .figures b {
    width: 44px;
    text-align: right;
  }
  /* Narrow lists give the name its room back, least important column first. */
  @container (max-width: 900px) {
    .row,
    :global(:root[data-density="compact"]) .row {
      grid-template-columns: 20px 30px minmax(120px, 1fr) 72px 128px minmax(110px, 150px) 82px 64px 28px;
    }
    .row .c-added {
      display: none;
    }
  }
  @container (max-width: 760px) {
    .row,
    :global(:root[data-density="compact"]) .row {
      grid-template-columns: 20px minmax(120px, 1fr) 72px 118px minmax(100px, 130px) 78px 28px;
    }
    .row .c-added,
    .row .c-eta,
    .row .c-tile {
      display: none;
    }
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 64px 24px;
    text-align: center;
  }
  .empty svg {
    margin-bottom: 10px;
    fill: none;
    stroke-width: 2.4;
  }
  .empty circle {
    stroke: var(--track);
  }
  .empty path {
    stroke: var(--accent);
    stroke-linecap: round;
  }
  .empty h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .empty p {
    margin: 0;
    max-width: 44ch;
  }
  .cta {
    display: flex;
    gap: 8px;
    margin: 14px 0 8px;
  }
  .tip {
    font-size: 12px;
  }
  .menuhead {
    padding: 6px 10px 4px;
    font-size: 11.5px;
  }
</style>
