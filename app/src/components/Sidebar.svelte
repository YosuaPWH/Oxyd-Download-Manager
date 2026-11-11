<script lang="ts">
  import * as fmt from "../lib/format";
  import { t, type Key } from "../lib/i18n.svelte";
  import { app, selectOnly, speedLog, type Library } from "../lib/store.svelte";
  import { type Category, type Queue } from "../lib/types";
  import Icon from "./Icon.svelte";
  import Sparkline from "./Sparkline.svelte";

  // The mark is a ring cut into eight segments, one of them lit: a file fetched in parallel.
  const arcs = [
    "M14 3A11 11 0 0 1 21.78 6.22",
    "M22.5 7A11 11 0 0 1 25 14",
    "M25 15A11 11 0 0 1 21.78 21.78",
    "M21 22.5A11 11 0 0 1 14 25",
    "M13 25A11 11 0 0 1 6.22 21.78",
    "M5.5 21A11 11 0 0 1 3 14",
    "M3 13A11 11 0 0 1 6.22 6.22",
    "M7 5.5A11 11 0 0 1 13 3",
  ];

  const libraries: { value: Library; key: Key; icon: string }[] = [
    { value: "all", key: "side.all", icon: "all" },
    { value: "unfinished", key: "nav.queued", icon: "clock" },
    { value: "finished", key: "nav.completed", icon: "completed" },
  ];
  const categories: Category[] = ["video", "music", "image", "document", "archive", "program", "other"];

  const counts = $derived.by(() => {
    const c: Record<string, number> = { all: 0, unfinished: 0, finished: 0, running: 0 };
    for (const k of categories) c[k] = 0;
    let speed = 0;
    for (const d of app.downloads) {
      c.all++;
      c[d.status === "completed" ? "finished" : "unfinished"]++;
      c[d.category]++;
      if (d.status === "downloading") {
        c.running++;
        speed += d.speed;
      }
    }
    return { c, speed };
  });

  // The readout splits the figure from its unit: "13.8" large, "MB/s" small.
  const readout = $derived((fmt.speed(counts.speed) || "0 B/s").split(" "));

  const perQueue = $derived.by(() => {
    const n = new Map<string, number>();
    for (const d of app.downloads) if (d.status !== "completed") n.set(d.queue, (n.get(d.queue) ?? 0) + 1);
    return n;
  });

  function goQueue(id: string) {
    app.library = "all";
    app.category = null;
    app.queue = id;
    app.page = "downloads";
    selectOnly(null);
  }

  function newQueue() {
    const blank: Queue = {
      id: "",
      name: "",
      maxActive: 2,
      speedLimit: null,
      schedule: { enabled: false, start: null, stop: null, days: [] },
    };
    app.queueEdit = blank;
  }

  function go(library: Library, category: Category | null) {
    app.library = library;
    app.category = category;
    app.queue = null;
    app.page = "downloads";
    selectOnly(null);
  }

  const onDownloads = $derived(app.page === "downloads");
</script>

<nav aria-label="Oxyd">
  <div class="brand">
    <svg class="mark" width="24" height="24" viewBox="0 0 28 28" role="img" aria-label="Oxyd">
      {#each arcs as d, i (i)}
        <path {d} class:lit={i === 0} />
      {/each}
    </svg>
    <span class="label">Oxyd</span>
  </div>

  {#each libraries as l (l.value)}
    <button
      class="item"
      title={t(l.key)}
      aria-current={onDownloads && !app.category && !app.queue && app.library === l.value ? "page" : undefined}
      onclick={() => go(l.value, null)}
    >
      <Icon name={l.icon} /><span class="label grow">{t(l.key)}</span><span class="num count">{counts.c[l.value]}</span>
    </button>
  {/each}

  <div class="caption label">{t("nav.categories")}</div>
  {#each categories as c (c)}
    <button
      class="item"
      title={t(`cat.${c}` as Key)}
      aria-current={onDownloads && app.category === c ? "page" : undefined}
      onclick={() => go("all", c)}
    >
      <Icon name={c} /><span class="label grow">{t(`cat.${c}` as Key)}</span><span class="num count">{counts.c[c]}</span>
    </button>
  {/each}

  <div class="caption label withadd">
    {t("queue.title")}
    <button class="icon add" title={t("queue.new")} aria-label={t("queue.new")} onclick={newQueue}><Icon name="add" size={13} /></button>
  </div>
  {#each app.queues as q (q.id)}
    <div class="queue">
      <button
        class="item"
        title={q.name || t("queue.main")}
        aria-current={onDownloads && app.queue === q.id ? "page" : undefined}
        onclick={() => goQueue(q.id)}
      >
        <Icon name={q.schedule.enabled ? "calendar" : "queue"} /><span class="label grow">{q.name || t("queue.main")}</span><span class="num count">{perQueue.get(q.id) ?? 0}</span>
      </button>
      <button class="icon edit label" title={t("queue.edit")} aria-label="{t('queue.edit')}: {q.name || t('queue.main')}" onclick={() => (app.queueEdit = q)}>
        <Icon name="edit" size={13} />
      </button>
    </div>
  {/each}
  {#if !app.queues.length}
    <!-- Before the list arrives the main queue still has its place. -->
    <button class="item" disabled><Icon name="queue" /><span class="label grow">{t("queue.main")}</span></button>
  {/if}

  <span class="spacer"></span>

  <div class="speed label" aria-label={t("side.totalSpeed")}>
    <div class="line">
      <span class="tiny">{t("side.totalSpeed")}</span>
      {#if counts.c.running}<span class="live">{counts.c.running} {t("status.active")}</span>{/if}
    </div>
    <div class="big num">{readout[0]} <small>{readout[1]}</small></div>
    <Sparkline samples={speedLog.total} />
    <div class="line faint">
      <span>
        {#if app.settings?.speedLimit}{t("status.limit")} <span class="num">{fmt.speed(app.settings.speedLimit)}</span>{:else}{t("status.noLimit")}{/if}
      </span>
      {#if speedLog.peak > 0}<span>{t("side.peak")} <span class="num">{fmt.speed(speedLog.peak)}</span></span>{/if}
    </div>
  </div>

  <button
    class="item"
    title="{t('tb.settings')} (Ctrl+,)"
    aria-current={app.page === "settings" ? "page" : undefined}
    onclick={() => (app.page = "settings")}
  >
    <Icon name="settings" /><span class="label grow">{t("tb.settings")}</span>
  </button>
</nav>

<style>
  nav {
    width: 216px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 8px 8px;
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--sheet-shadow);
    backdrop-filter: blur(24px) saturate(160%);
    overflow-y: auto;
    scrollbar-width: none;
  }
  :global(.no-glass) nav {
    background: var(--surface);
    backdrop-filter: none;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 8px 12px;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .mark {
    flex: none;
    fill: none;
    stroke: var(--text);
    stroke-width: 3;
  }
  .mark .lit {
    stroke: var(--accent);
  }
  .item {
    flex: none;
    justify-content: flex-start;
    gap: 10px;
    width: 100%;
    padding: 0 8px;
    border: 0;
    border-radius: 7px;
    color: var(--muted);
    font-weight: 500;
  }
  .item :global(svg) {
    flex: none;
  }
  .item:hover:not(:disabled) {
    color: var(--text);
  }
  .item[aria-current="page"] {
    color: var(--text);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  .item[aria-current="page"] :global(svg) {
    color: var(--accent);
  }
  .grow {
    flex: 1;
    min-width: 0;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .count {
    font-size: 11.5px;
    color: var(--faint);
  }
  .caption {
    padding: 14px 8px 4px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .withadd {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-right: 2px;
  }
  .add {
    width: 20px;
    height: 20px;
  }
  .queue {
    position: relative;
    flex: none;
  }
  .edit {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 24px;
    height: 24px;
    opacity: 0;
    background: var(--raised);
  }
  .queue:hover .edit,
  .edit:focus-visible {
    opacity: 1;
  }
  .queue:hover .count {
    visibility: hidden;
  }
  .spacer {
    flex: 1;
    min-height: 12px;
  }
  .speed {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 6px;
    padding: 10px 12px 8px;
    border-radius: 10px;
    background: var(--sunken);
    box-shadow: inset 0 0 0 1px var(--rule);
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 11px;
    line-height: 16px;
    white-space: nowrap;
  }
  .line .num {
    font-size: 11px;
  }
  .tiny {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .live {
    color: var(--accent);
  }
  .big {
    font-size: 24px;
    line-height: 30px;
    font-weight: 600;
    letter-spacing: -0.03em;
  }
  .big small {
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0;
    color: var(--muted);
  }
  /* Narrow windows: icons only. */
  @media (max-width: 940px) {
    nav {
      width: 52px;
      align-items: center;
    }
    .label,
    .count {
      display: none;
    }
    .brand {
      padding: 2px 0 12px;
    }
    .item {
      width: 36px;
      padding: 0;
      justify-content: center;
    }
  }
</style>
