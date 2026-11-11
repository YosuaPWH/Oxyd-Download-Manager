<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorMessage } from "./lib/api";
  import { parseBatch } from "./lib/batch";
  import * as fmt from "./lib/format";
  import { t } from "./lib/i18n.svelte";
  import { app, init, run, speedLog } from "./lib/store.svelte";
  import Icon from "./components/Icon.svelte";
  import Sparkline from "./components/Sparkline.svelte";

  // The mini window: always on top, shows what is moving and takes dropped links.
  onMount(() => {
    void run(init);
  });

  const running = $derived(app.downloads.filter((d) => d.status === "downloading"));
  const waiting = $derived(app.downloads.filter((d) => d.status === "queued" || d.status === "probing").length);
  const speed = $derived(running.reduce((s, d) => s + d.speed, 0));
  const readout = $derived((fmt.speed(speed) || "0 B/s").split(" "));
  // The one that ends first is the one worth a line.
  const lead = $derived([...running].sort((a, b) => (a.eta ?? Infinity) - (b.eta ?? Infinity))[0]);
  const leadPct = $derived(lead ? fmt.percent(lead.downloaded, lead.totalSize) : null);

  let over = $state(0);
  let note = $state<string | null>(null);

  function say(text: string) {
    note = text;
    setTimeout(() => (note = null), 3500);
  }

  /** Dropped links start right away: the mini window has no dialogs. */
  async function drop(e: DragEvent) {
    e.preventDefault();
    over = 0;
    const text = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "";
    const magnets = text.split(/\s+/).filter((w) => /^magnet:/i.test(w));
    const urls = [...parseBatch(text.replace(/^#.*$/gm, "")).urls, ...magnets];
    if (!urls.length) return say(t("drop.none"));
    let added = 0;
    let failure: string | null = null;
    for (const url of urls) {
      try {
        await api.add({ url, start: true });
        added++;
      } catch (err) {
        failure ??= errorMessage(err);
      }
    }
    say(added ? `${added} ${t("batch.added")}` : (failure ?? t("drop.none")));
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="mini"
  class:over={over > 0}
  ondragenter={() => over++}
  ondragleave={() => (over = Math.max(0, over - 1))}
  ondragover={(e) => e.preventDefault()}
  ondrop={drop}
  ondblclick={() => void api.showMain()}
>
  <header data-tauri-drag-region>
    <span class="readout num" data-tauri-drag-region>{readout[0]} <small>{readout[1]}</small></span>
    <span class="grow" data-tauri-drag-region></span>
    <button class="icon" title={t("mini.open")} aria-label={t("mini.open")} onclick={() => api.showMain()}><Icon name="panel" size={14} /></button>
    <button class="icon" title={t("mini.close")} aria-label={t("mini.close")} onclick={() => api.toggleMini()}><Icon name="x" size={14} /></button>
  </header>
  <Sparkline samples={speedLog.total} height={30} />
  <div class="line">
    {#if note}
      <span class="ellipsis accent">{note}</span>
    {:else if over > 0}
      <span class="accent">{t("drop.title")}</span>
    {:else if lead}
      <span class="ellipsis name">{lead.filename}</span>
      <span class="num">{leadPct === null ? fmt.bytes(lead.downloaded) : `${leadPct.toFixed(0)}%`}</span>
    {:else}
      <span class="faint">{waiting ? `${waiting} ${t("sum.queued")}` : t("mini.idle")}</span>
    {/if}
  </div>
  <div class="track"><div class="fill" style:width="{lead ? (leadPct ?? 0) : 0}%"></div></div>
  <div class="line faint small">
    <span><span class="num">{running.length}</span> {t("status.active")}{#if waiting} · <span class="num">{waiting}</span> {t("sum.queued")}{/if}</span>
    {#if lead?.eta != null}<span class="num">{fmt.duration(lead.eta)}</span>{/if}
  </div>
</div>

<style>
  .mini {
    display: flex;
    flex-direction: column;
    gap: 4px;
    height: 100vh;
    padding: 6px 12px 10px;
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--border);
    user-select: none;
    cursor: default;
  }
  .mini.over {
    box-shadow: inset 0 0 0 2px var(--accent);
    background: var(--accent-softer);
  }
  header {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .readout {
    font-size: 20px;
    line-height: 28px;
    font-weight: 600;
    letter-spacing: -0.03em;
  }
  .readout small {
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0;
    color: var(--muted);
  }
  .grow {
    flex: 1;
    align-self: stretch;
  }
  header .icon {
    width: 24px;
    height: 24px;
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    min-height: 18px;
    font-size: 12px;
    line-height: 18px;
  }
  .small {
    font-size: 11px;
  }
  .small .num,
  .line .num {
    font-size: 11px;
  }
  .name {
    min-width: 0;
  }
  .accent {
    color: var(--accent);
  }
  .track {
    height: 3px;
    border-radius: 2px;
    background: var(--track);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.25s linear;
  }
</style>
