<script lang="ts">
  import { api, errorMessage } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t, type Key } from "../lib/i18n.svelte";
  import { CONVERT_FORMATS, type DownloadInfo, type ToolJob } from "../lib/types";
  import Modal from "./Modal.svelte";

  let { downloads, onclose }: { downloads: DownloadInfo[]; onclose: () => void } = $props();

  type Tab = "convert" | "shrink" | "zip";
  const media = $derived(downloads.filter((d) => d.category === "video" || d.category === "music"));
  const videos = $derived(downloads.filter((d) => d.category === "video"));
  // svelte-ignore state_referenced_locally
  let tab = $state<Tab>(media.length ? "convert" : "zip");
  let format = $state<(typeof CONVERT_FORMATS)[number]>("mp4");
  let megabytes = $state(25);
  // svelte-ignore state_referenced_locally
  let zipName = $state(
    downloads.length === 1 ? downloads[0].filename.replace(/\.[^.]+$/, "") : t("tools.zipDefault"),
  );
  let error = $state<string | null>(null);
  let busy = $state(false);

  const tabs: { value: Tab; key: Key; n: number }[] = $derived([
    { value: "convert", key: "tools.convert", n: media.length },
    { value: "shrink", key: "tools.shrink", n: videos.length },
    { value: "zip", key: "tools.zip", n: downloads.length },
  ]);
  /** What the chosen tool works on. */
  const targets = $derived(tab === "convert" ? media : tab === "shrink" ? videos : downloads);
  const total = $derived(targets.reduce((s, d) => s + (d.totalSize ?? 0), 0));

  async function run() {
    const job: ToolJob =
      tab === "convert"
        ? { type: "convert", format }
        : tab === "shrink"
          ? { type: "shrink", megabytes: Math.max(1, Math.round(megabytes)) }
          : { type: "zip", name: zipName.trim() || t("tools.zipDefault") };
    busy = true;
    error = null;
    try {
      await api.runTool(targets.map((d) => d.id), job);
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={t("tools.title")} {onclose} width={480}>
  <div class="stack">
    <div class="lines" role="tablist">
      {#each tabs as x (x.value)}
        <button role="tab" aria-selected={tab === x.value} disabled={!x.n} onclick={() => (tab = x.value)}>{t(x.key)}</button>
      {/each}
    </div>

    {#if tab === "convert"}
      <div>
        <span class="label" id="t-format">{t("tools.format")}</span>
        <div class="formats" role="radiogroup" aria-labelledby="t-format">
          {#each CONVERT_FORMATS as f (f)}
            <button role="radio" aria-checked={format === f} onclick={() => (format = f)}>{f.toUpperCase()}</button>
          {/each}
        </div>
        <p class="faint hint">{t(`tools.hint.${format}` as Key)}</p>
      </div>
    {:else if tab === "shrink"}
      <div>
        <label class="label" for="t-mb">{t("tools.target")}</label>
        <div class="row">
          <input id="t-mb" class="num" type="number" min="1" max="100000" bind:value={megabytes} />
          <span class="muted">MB</span>
          {#each [10, 25, 50, 100] as mb (mb)}
            <button type="button" class="preset" onclick={() => (megabytes = mb)}>{mb}</button>
          {/each}
        </div>
        <p class="faint hint">{t("tools.shrinkHint")}</p>
      </div>
    {:else}
      <div>
        <label class="label" for="t-zip">{t("tools.zipName")}</label>
        <div class="row"><input id="t-zip" class="grow" bind:value={zipName} /><span class="muted mono">.zip</span></div>
        <p class="faint hint">{t("tools.zipHint")}</p>
      </div>
    {/if}

    <ul>
      {#each targets.slice(0, 4) as d (d.id)}
        <li class="ellipsis">{d.filename}</li>
      {/each}
      {#if targets.length > 4}<li class="muted">+<span class="num">{targets.length - 4}</span> {t("new.more")}</li>{/if}
    </ul>
    {#if error}<div class="errbox" role="alert">{error}</div>{/if}
  </div>

  {#snippet footer()}
    <span class="muted count"><span class="num">{targets.length}</span> {t("tools.files")} · <span class="num">{fmt.bytes(total)}</span></span>
    <button onclick={onclose}>{t("new.cancel")}</button>
    <button class="primary" onclick={run} disabled={busy || !targets.length}>{t("tools.start")}</button>
  {/snippet}
</Modal>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .lines {
    align-self: flex-start;
  }
  .label {
    display: block;
    margin-bottom: 6px;
    font-size: 12px;
    line-height: 16px;
    color: var(--muted);
  }
  .formats {
    display: flex;
    gap: 6px;
  }
  .formats button {
    min-width: 58px;
    font-family: var(--font-num);
    font-size: 12px;
    border-color: var(--border);
    color: var(--muted);
  }
  .formats button[aria-checked="true"] {
    color: var(--accent);
    font-weight: 600;
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  #t-mb {
    width: 96px;
    text-align: right;
  }
  .grow {
    flex: 1;
  }
  .preset {
    padding: 0 9px;
    border-color: var(--border);
    color: var(--muted);
    font-family: var(--font-num);
    font-size: 12px;
  }
  .hint {
    margin: 8px 0 0;
    font-size: 12px;
  }
  ul {
    margin: 0;
    padding: 10px 14px;
    list-style: none;
    border-radius: var(--radius);
    background: var(--raised);
  }
  li {
    line-height: 22px;
  }
  .errbox {
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--neg-bg);
    color: var(--negative);
  }
  .count {
    margin-right: auto;
    align-self: center;
  }
</style>
