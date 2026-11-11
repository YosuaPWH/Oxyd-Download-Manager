<script lang="ts">
  import { api } from "../lib/api";
  import { t } from "../lib/i18n.svelte";
  import { run } from "../lib/store.svelte";
  import type { DownloadInfo } from "../lib/types";
  import Modal from "./Modal.svelte";

  let { downloads, onclose }: { downloads: DownloadInfo[]; onclose: () => void } = $props();
  let deleteFile = $state(false);

  const hasFiles = $derived(downloads.some((d) => d.status === "completed"));

  function confirm() {
    const list = downloads.map((d) => ({ id: d.id, done: d.status === "completed" }));
    const del = deleteFile;
    onclose();
    void run(async () => {
      for (const d of list) await api.remove(d.id, del && d.done);
    });
  }
</script>

<Modal title={downloads.length > 1 ? `${t("rm.titleMany")} (${downloads.length})` : t("rm.title")} {onclose} width={440}>
  <ul>
    {#each downloads.slice(0, 5) as d (d.id)}
      <li class="ellipsis">{d.filename || d.url}</li>
    {/each}
    {#if downloads.length > 5}
      <li class="muted">+<span class="num">{downloads.length - 5}</span> {t("new.more")}</li>
    {/if}
  </ul>
  {#if hasFiles}
    <label class="check"><input type="checkbox" bind:checked={deleteFile} />{t("rm.deleteFile")}</label>
  {/if}
  {#snippet footer()}
    <button onclick={onclose}>{t("set.cancel")}</button>
    <button class="primary" onclick={confirm}>{t("rm.confirm")}</button>
  {/snippet}
</Modal>

<style>
  ul {
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
  }
  li {
    line-height: 22px;
    font-weight: 500;
  }
  li.muted {
    font-weight: 400;
  }
</style>
