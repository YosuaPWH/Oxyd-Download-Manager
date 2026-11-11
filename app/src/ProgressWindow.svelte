<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api } from "./lib/api";
  import { canPause, canStart, hostOf } from "./lib/actions";
  import * as fmt from "./lib/format";
  import { detailText, errorText, t, type Key } from "./lib/i18n.svelte";
  import { app, clock, init, run } from "./lib/store.svelte";
  import SegmentBar from "./components/SegmentBar.svelte";

  // The progress window of one download: what it does now, and the few things to do about it.
  let { id }: { id: string } = $props();

  let loaded = $state(false);
  onMount(() => {
    void run(async () => {
      await init();
      loaded = true;
    });
  });

  const d = $derived(app.downloads.find((x) => x.id === id));
  const pct = $derived(d ? fmt.percent(d.downloaded, d.totalSize) : null);
  const moving = $derived(d?.status === "downloading");
  const done = $derived(d?.status === "completed");

  const close = () => void getCurrentWindow().close();

  // The title bar carries the progress, so it is readable from the taskbar.
  $effect(() => {
    if (!d) return;
    const head = pct === null || done ? "" : `${pct.toFixed(0)}% · `;
    void getCurrentWindow().setTitle(`${head}${d.filename || "Oxyd"}`).catch(() => {});
  });

  // A download removed from the list has nothing left to show.
  $effect(() => {
    if (loaded && !d) close();
  });

  function status() {
    if (!d) return "";
    const doing = d.detail && d.detail !== "ytdlp" ? detailText(d.detail) : null;
    if (doing && !done) return doing;
    if (d.status === "failed" && d.retryAt) return `${t("st.failed")} · ↻ ${fmt.duration(fmt.until(d.retryAt, clock.now)) || "0s"}`;
    return t(`st.${d.status}` as Key);
  }
</script>

<main class={d?.status}>
  {#if d}
    <header>
      <h1 class="ellipsis" title={d.filename || d.url}>{d.filename || d.url}</h1>
      <p class="faint ellipsis">{d.kind === "torrent" ? "BitTorrent" : hostOf(d)}{d.savePath ? ` → ${d.dir}` : ""}</p>
    </header>

    <div class="figures">
      <span class="pct num">{pct === null ? fmt.bytes(d.downloaded) : `${pct.toFixed(1)}%`}</span>
      <span class="state">{status()}</span>
      <span class="grow"></span>
      <span class="num">{fmt.bytes(d.downloaded)}{#if d.totalSize !== null}<span class="faint"> / {fmt.bytes(d.totalSize)}</span>{/if}</span>
    </div>

    <SegmentBar segments={d.segments} total={d.totalSize} downloaded={d.downloaded} />

    <dl>
      <div><dt>{t("col.speed")}</dt><dd class="num">{moving ? fmt.speed(d.speed) || "0 B/s" : "—"}</dd></div>
      <div><dt>{t("col.eta")}</dt><dd class="num">{moving && d.eta !== null ? fmt.duration(d.eta) : "—"}</dd></div>
      <div>
        <dt>{t(d.kind === "torrent" ? "insp.peers" : "detail.connections")}</dt>
        <dd class="num">{moving ? d.activeConnections : "—"}</dd>
      </div>
    </dl>

    {#if d.error && !done}
      <p class="error" role="alert">{errorText(d.errorCode) ?? d.error}</p>
    {/if}

    <footer>
      {#if done}
        <button class="primary" onclick={() => run(() => api.openFile(id))}>{t("act.open")}</button>
        <button onclick={() => run(() => api.openFolder(id))}>{t("act.folder")}</button>
      {:else}
        {#if canPause(d)}
          <button class="primary" onclick={() => run(() => api.pause(id))}>{t("act.pause")}</button>
        {:else if canStart(d)}
          <button class="primary" onclick={() => run(() => api.start(id))}>{t("act.resume")}</button>
        {/if}
        {#if d.status !== "cancelled"}
          <button onclick={() => run(() => api.cancel(id))}>{t("act.cancel")}</button>
        {/if}
      {/if}
      <span class="grow"></span>
      <button class="ghost" onclick={() => api.showMain()}>{t("mini.open")}</button>
      <button onclick={close}>{t("progress.close")}</button>
    </footer>
  {:else if loaded}
    <p class="muted">{t("progress.gone")}</p>
  {/if}
  {#if app.error}<p class="error" role="alert">{app.error}</p>{/if}
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    gap: 12px;
    height: 100vh;
    padding: 16px 18px 14px;
    background: var(--surface);
    --state: var(--faint);
  }
  main.downloading {
    --state: var(--accent);
  }
  main.paused {
    --state: var(--warning);
  }
  main.failed {
    --state: var(--negative);
  }
  main.completed {
    --state: var(--positive);
  }
  h1 {
    margin: 0;
    font-size: 15px;
    line-height: 20px;
    font-weight: 600;
  }
  header p {
    margin: 0;
    font-size: 12px;
  }
  .figures {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .pct {
    font-size: 26px;
    line-height: 30px;
    font-weight: 600;
    letter-spacing: -0.03em;
  }
  .state {
    color: var(--state);
    font-weight: 500;
  }
  .grow {
    flex: 1;
  }
  dl {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
    margin: 0;
  }
  dt {
    font-size: 11px;
    color: var(--faint);
  }
  dd {
    margin: 0;
    font-size: 13px;
  }
  .error {
    margin: 0;
    font-size: 12px;
    color: var(--negative);
  }
  footer {
    display: flex;
    gap: 8px;
    margin-top: auto;
  }
</style>
