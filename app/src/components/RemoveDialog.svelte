<script lang="ts">
  import { api } from "../lib/api";
  import { t } from "../lib/i18n.svelte";
  import { run } from "../lib/store.svelte";
  import type { DownloadInfo } from "../lib/types";
  import Modal from "./Modal.svelte";

  let { download, onclose }: { download: DownloadInfo; onclose: () => void } = $props();
  let deleteFile = $state(false);

  function confirm() {
    const id = download.id;
    const del = deleteFile;
    onclose();
    void run(() => api.remove(id, del));
  }
</script>

<Modal title={t("rm.title")} {onclose} width={440}>
  <p class="name ellipsis">{download.filename || download.url}</p>
  {#if download.status === "completed"}
    <label class="check"><input type="checkbox" bind:checked={deleteFile} />{t("rm.deleteFile")}</label>
  {/if}
  {#snippet footer()}
    <button onclick={onclose}>{t("set.cancel")}</button>
    <button class="primary" onclick={confirm}>{t("rm.confirm")}</button>
  {/snippet}
</Modal>

<style>
  .name {
    margin: 0 0 12px;
    font-weight: 600;
  }
</style>
