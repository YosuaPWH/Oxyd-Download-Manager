<script lang="ts">
  import { api, errorMessage } from "../lib/api";
  import { t, type Key } from "../lib/i18n.svelte";
  import { app } from "../lib/store.svelte";
  import { MAIN_QUEUE, type Category, type GrabResult } from "../lib/types";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let url = $state("");
  let depth = $state(0);
  let sameSite = $state(true);
  let scanning = $state(false);
  let result = $state<GrabResult | null>(null);
  let error = $state<string | null>(null);
  let type = $state<Category | null>(null);
  let skipped = $state<string[]>([]);
  let queue = $state(MAIN_QUEUE);
  let busy = $state(false);
  let done = $state(0);

  const SHOWN = 300;
  const types = $derived.by(() => {
    const count = new Map<Category, number>();
    for (const i of result?.items ?? []) count.set(i.category, (count.get(i.category) ?? 0) + 1);
    return [...count.entries()];
  });
  const listed = $derived((result?.items ?? []).filter((i) => !type || i.category === type));
  const chosen = $derived(listed.filter((i) => !skipped.includes(i.url)));

  async function scan() {
    const target = url.trim();
    if (!target) return;
    scanning = true;
    error = null;
    result = null;
    type = null;
    skipped = [];
    try {
      result = await api.grabPage(/^https?:\/\//i.test(target) ? target : `https://${target}`, { depth, sameSite });
    } catch (e) {
      error = errorMessage(e);
    } finally {
      scanning = false;
    }
  }

  function toggle(u: string) {
    skipped = skipped.includes(u) ? skipped.filter((x) => x !== u) : [...skipped, u];
  }

  function toggleAll() {
    const urls = listed.map((i) => i.url);
    skipped = chosen.length ? [...new Set([...skipped, ...urls])] : skipped.filter((u) => !urls.includes(u));
  }

  async function add(start: boolean) {
    busy = true;
    error = null;
    done = 0;
    let failed = 0;
    let first: string | null = null;
    for (const item of chosen) {
      try {
        // The page is sent as the referer: many sites refuse their files without it.
        await api.add({ url: item.url, start, queue, headers: { referer: item.page } });
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

<Modal title={t("grab.title")} {onclose} width={680}>
  <form class="stack" onsubmit={(e) => (e.preventDefault(), void scan())}>
    <div>
      <label for="g-url">{t("grab.url")}</label>
      <div class="row">
        <input id="g-url" class="mono" bind:value={url} placeholder="https://example.org/gallery/" />
        <button type="submit" class:primary={!result} disabled={scanning || !url.trim()}>
          <Icon name="search" size={14} />{scanning ? t("grab.scanning") : t("grab.scan")}
        </button>
      </div>
    </div>
    <div class="options">
      <label for="g-depth">{t("grab.depth")}</label>
      <select id="g-depth" bind:value={depth}>
        <option value={0}>{t("grab.depth0")}</option>
        <option value={1}>{t("grab.depth1")}</option>
        <option value={2}>{t("grab.depth2")}</option>
      </select>
      <label class="check"><input type="checkbox" bind:checked={sameSite} disabled={depth === 0} />{t("grab.sameSite")}</label>
    </div>

    {#if result}
      <div class="chips" role="tablist" aria-label={t("nav.categories")}>
        <button type="button" role="tab" aria-selected={type === null} onclick={() => (type = null)}>
          {t("nav.all")}<span class="num">{result.items.length}</span>
        </button>
        {#each types as [c, n] (c)}
          <button type="button" role="tab" aria-selected={type === c} onclick={() => (type = c)}>
            {t(`cat.${c}` as Key)}<span class="num">{n}</span>
          </button>
        {/each}
        <span class="faint pages">
          <span class="num">{result.pages}</span> {t("grab.pages")}{#if result.skipped}, <span class="num">{result.skipped}</span> {t("grab.skipped")}{/if}
        </span>
      </div>
      {#if listed.length}
        <div class="list" role="list">
          <label class="item head">
            <input type="checkbox" checked={chosen.length === listed.length} onchange={toggleAll} />
            <span class="name">{t("list.selectAll")}</span>
          </label>
          {#each listed.slice(0, SHOWN) as item (item.url)}
            <label class="item" role="listitem" title={item.url}>
              <input type="checkbox" checked={!skipped.includes(item.url)} onchange={() => toggle(item.url)} />
              <Icon name={item.category} size={14} />
              <span class="ellipsis name mono">{item.name}</span>
              <span class="ellipsis faint host">{new URL(item.url).host}</span>
            </label>
          {/each}
          {#if listed.length > SHOWN}
            <div class="item faint">+<span class="num">{listed.length - SHOWN}</span> {t("new.more")}</div>
          {/if}
        </div>
      {:else}
        <p class="muted none">{t("grab.none")}</p>
      {/if}
      {#if result.truncated}<div class="note" role="status">{t("grab.truncated")}</div>{/if}
    {:else if !scanning && !error}
      <p class="faint none">{t("grab.hint")}</p>
    {/if}
    {#if error}<div class="errbox" role="alert">{error}</div>{/if}
  </form>

  {#snippet footer()}
    <span class="muted count" aria-live="polite">
      {#if busy}
        {t("batch.adding")} <span class="num">{done}</span> / <span class="num">{chosen.length}</span>
      {:else if result}
        <span class="num">{chosen.length}</span> {t("sel.count")}
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
    <button onclick={() => add(false)} disabled={busy || !chosen.length}>{t("new.later")}</button>
    <button class="primary" onclick={() => add(true)} disabled={busy || !chosen.length}>
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
    font-size: 12px;
    color: var(--muted);
  }
  .stack > div > label[for] {
    display: block;
    margin-bottom: 6px;
    line-height: 16px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row input {
    flex: 1;
  }
  .options {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 14px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .chips button {
    height: 24px;
    padding: 0 9px;
    gap: 6px;
    border-radius: 999px;
    font-size: 12px;
    color: var(--muted);
    border-color: var(--border);
  }
  .chips button[aria-selected="true"] {
    color: var(--accent);
    font-weight: 600;
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
  .chips .num {
    font-size: 11px;
  }
  .pages {
    margin-left: auto;
    font-size: 12px;
  }
  .list {
    max-height: 260px;
    overflow-y: auto;
    border-radius: var(--radius);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 28px;
    padding: 0 10px;
    border-bottom: 1px solid var(--rule);
    cursor: pointer;
  }
  .item:last-child {
    border-bottom: 0;
  }
  .item.head {
    position: sticky;
    top: 0;
    background: var(--raised);
    font-size: 12px;
    color: var(--muted);
  }
  .item input {
    margin: 0;
  }
  .item :global(svg) {
    flex: none;
    color: var(--faint);
  }
  .name {
    flex: 1;
    min-width: 0;
  }
  .host {
    max-width: 38%;
    font-size: 12px;
  }
  .none {
    margin: 0;
  }
  .note {
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--warn-bg);
    color: var(--warning);
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
