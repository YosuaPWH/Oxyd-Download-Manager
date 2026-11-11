<script lang="ts">
  import { api, errorMessage } from "../lib/api";
  import { BATCH_LIMIT, linkName, parseBatch } from "../lib/batch";
  import { t } from "../lib/i18n.svelte";
  import { app } from "../lib/store.svelte";
  import { MAIN_QUEUE } from "../lib/types";
  import Modal from "./Modal.svelte";

  let { initial = "", onclose }: { initial?: string; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(initial);
  let dir = $state("");
  let skipped = $state<string[]>([]);
  let busy = $state(false);
  let done = $state(0);
  let error = $state<string | null>(null);
  let queue = $state(app.queue ?? MAIN_QUEUE);

  const SHOWN = 80;
  const batch = $derived(parseBatch(text));
  const chosen = $derived(batch.urls.filter((u) => !skipped.includes(u)));

  function toggle(url: string) {
    skipped = skipped.includes(url) ? skipped.filter((u) => u !== url) : [...skipped, url];
  }

  async function browse() {
    const picked = await api.pickFolder(dir || app.settings?.downloadDir || null);
    if (picked) dir = picked;
  }

  async function submit(start: boolean) {
    busy = true;
    error = null;
    done = 0;
    let failed = 0;
    let first: string | null = null;
    for (const url of chosen) {
      try {
        // No folder chosen: the app picks it per file, as for a single download.
        await api.add({ url, dir: dir || null, start, queue });
      } catch (e) {
        failed++;
        first ??= errorMessage(e);
      }
      done++;
    }
    busy = false;
    const added = chosen.length - failed;
    if (added) app.notice = `${added} ${t("batch.added")}`;
    if (failed) error = `${failed} ${t("batch.failed")} ${first ?? ""}`;
    else onclose();
  }
</script>

<Modal title={t("batch.title")} {onclose} width={620}>
  <div class="stack">
    <div>
      <label for="b-text">{t("batch.links")}</label>
      <textarea
        id="b-text"
        class="mono"
        rows="5"
        spellcheck="false"
        bind:value={text}
        placeholder={"https://example.org/photo-[001-120].jpg\nhttps://example.org/disk-[a-c].iso"}
      ></textarea>
      <p class="hint faint">{t("batch.hint")}</p>
    </div>

    {#if batch.urls.length}
      <div class="list" role="list" aria-label={t("batch.preview")}>
        {#each batch.urls.slice(0, SHOWN) as url (url)}
          <label class="link" role="listitem" title={url}>
            <input type="checkbox" checked={!skipped.includes(url)} onchange={() => toggle(url)} />
            <span class="ellipsis name">{linkName(url)}</span>
            <span class="ellipsis faint host">{new URL(url).host}</span>
          </label>
        {/each}
        {#if batch.urls.length > SHOWN}
          <div class="link faint">+<span class="num">{batch.urls.length - SHOWN}</span> {t("new.more")}</div>
        {/if}
      </div>
    {/if}

    <div>
      <label for="b-dir">{t("new.folder")}</label>
      <div class="row">
        <input id="b-dir" bind:value={dir} placeholder={t("batch.autoFolder")} />
        <button type="button" onclick={browse}>{t("new.browse")}</button>
      </div>
    </div>

    {#if batch.truncated}
      <div class="note warn" role="status">{t("batch.truncated")} <span class="num">{BATCH_LIMIT}</span>.</div>
    {/if}
    {#if error}
      <div class="note bad" role="alert">{error}</div>
    {/if}
  </div>

  {#snippet footer()}
    <span class="muted count" aria-live="polite">
      {#if busy}
        {t("batch.adding")} <span class="num">{done}</span> / <span class="num">{chosen.length}</span>
      {:else}
        <span class="num">{chosen.length}</span> {t("batch.count")}
        {#if batch.duplicates}· <span class="num">{batch.duplicates}</span> {t("batch.duplicates")}{/if}
        {#if batch.invalid}· <span class="num">{batch.invalid}</span> {t("batch.invalid")}{/if}
      {/if}
    </span>
    {#if app.queues.length > 1}
      <select aria-label={t("queue.label")} bind:value={queue}>
        {#each app.queues as q (q.id)}
          <option value={q.id}>{q.name || t("queue.main")}</option>
        {/each}
      </select>
    {/if}
    <button onclick={onclose}>{t("new.cancel")}</button>
    <button onclick={() => submit(false)} disabled={busy || !chosen.length}>{t("new.later")}</button>
    <button class="primary" onclick={() => submit(true)} disabled={busy || !chosen.length}>
      {t("batch.add")}{chosen.length ? ` ${chosen.length}` : ""}
    </button>
  {/snippet}
</Modal>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  label[for] {
    display: block;
    margin-bottom: 6px;
    font-size: 12px;
    line-height: 16px;
    color: var(--muted);
  }
  textarea {
    display: block;
    width: 100%;
    line-height: 19px;
    white-space: pre;
  }
  .hint {
    margin: 6px 0 0;
    font-size: 12px;
  }
  .list {
    max-height: 190px;
    overflow-y: auto;
    border-radius: var(--radius);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .link {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 28px;
    padding: 0 10px;
    border-bottom: 1px solid var(--rule);
    cursor: pointer;
  }
  .link:last-child {
    border-bottom: 0;
  }
  .link input {
    margin: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
    font-family: var(--font-num);
    font-size: 12px;
  }
  .host {
    max-width: 40%;
    font-size: 12px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row input {
    flex: 1;
  }
  .note {
    padding: 8px 12px;
    border-radius: var(--radius);
  }
  .note.warn {
    background: var(--warn-bg);
    color: var(--warning);
  }
  .note.bad {
    background: var(--neg-bg);
    color: var(--negative);
  }
  .count {
    margin-right: auto;
    align-self: center;
  }
</style>
