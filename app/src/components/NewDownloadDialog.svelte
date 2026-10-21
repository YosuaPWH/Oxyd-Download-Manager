<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorMessage } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { app } from "../lib/store.svelte";
  import type { DuplicatePolicy, PendingAdd, PendingMatch, ProbeInfo } from "../lib/types";
  import Modal from "./Modal.svelte";

  let {
    pending = null,
    initialUrl = "",
    onclose,
  }: { pending?: PendingAdd | null; initialUrl?: string; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let url = $state(pending?.url ?? initialUrl);
  // svelte-ignore state_referenced_locally
  let filename = $state(pending?.filename ?? "");
  let dir = $state("");
  let dirTouched = false;
  let nameTouched = false;
  let probe = $state<ProbeInfo | null>(null);
  let probing = $state(false);
  let error = $state<string | null>(null);
  let exists = $state(false);
  let conflict = $state<DuplicatePolicy>("rename");
  let connections = $state(app.settings?.connections ?? 8);
  let checksum = $state("");
  let busy = $state(false);
  /** Unfinished download this link continues (from the browser match or after probing). */
  // svelte-ignore state_referenced_locally
  let match = $state<PendingMatch | null>(pending?.matched ?? null);

  const size = $derived(probe?.size ?? pending?.size ?? null);

  async function check() {
    const target = url.trim();
    if (!target) return;
    probing = true;
    error = null;
    try {
      probe = pending ? await api.probePending(pending.key) : await api.probe(target);
      if (!nameTouched && probe.filename && (!filename || !pending?.filename)) {
        filename = probe.filename;
      }
      const name = filename || probe.filename;
      if (!match && name) {
        const d = await api.findUnfinished(name, probe.size);
        match = d
          ? { id: d.id, filename: d.filename, downloaded: d.downloaded, totalSize: d.totalSize }
          : null;
      }
    } catch (e) {
      error = errorMessage(e);
      probe = null;
    } finally {
      probing = false;
    }
  }

  // Suggest a folder for the file name and detect name clashes.
  $effect(() => {
    const name = filename;
    if (!dirTouched) {
      void api.suggestDir(name).then((d) => {
        if (!dirTouched) dir = d;
      });
    }
  });

  $effect(() => {
    const [d, n] = [dir, filename];
    if (!d || !n) {
      exists = false;
      return;
    }
    void api.pathExists(d, n).then((e) => (exists = e));
  });

  onMount(() => {
    if (url) void check();
  });

  async function browse() {
    const picked = await api.pickFolder(dir || null);
    if (picked) {
      dir = picked;
      dirTouched = true;
    }
  }

  async function submit(start: boolean) {
    busy = true;
    error = null;
    const onConflict: DuplicatePolicy | null = exists ? conflict : null;
    try {
      if (pending) {
        await api.confirmPending(pending.key, {
          filename: filename.trim() || null,
          dir: dir || null,
          start,
          connections,
          onConflict,
          sizeHint: size,
          checksum: checksum.trim() || null,
        });
      } else {
        await api.add({
          url: url.trim(),
          filename: filename.trim() || null,
          dir: dir || null,
          connections,
          start,
          onConflict,
          sizeHint: size,
          checksum: checksum.trim() || null,
        });
      }
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  /** Give the unfinished download this link and resume it. */
  async function continueExisting() {
    if (!match) return;
    busy = true;
    error = null;
    try {
      if (pending) await api.resumePending(pending.key, match.id);
      else await api.resumeWithUrl(match.id, url.trim());
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function cancel() {
    if (pending) void api.discardPending(pending.key);
    onclose();
  }

  const more = $derived(Math.max(0, app.pending.length - 1));
</script>

<Modal title={t("new.title")} onclose={cancel} width={600}>
  <form class="grid" onsubmit={(e) => (e.preventDefault(), submit(true))}>
    <label for="nd-url">{t("new.url")}</label>
    <div class="row">
      <input
        id="nd-url"
        class="mono"
        bind:value={url}
        readonly={!!pending}
        placeholder="https://"
        onchange={check}
      />
      {#if !pending}
        <button type="button" onclick={check} disabled={probing || !url.trim()}>{t("new.check")}</button>
      {/if}
    </div>

    <label for="nd-name">{t("new.filename")}</label>
    <input id="nd-name" bind:value={filename} oninput={() => (nameTouched = true)} />

    <label for="nd-dir">{t("new.folder")}</label>
    <div class="row">
      <input id="nd-dir" bind:value={dir} oninput={() => (dirTouched = true)} />
      <button type="button" onclick={browse}>{t("new.browse")}</button>
    </div>

    <span class="lbl">{t("new.size")}</span>
    <div class="info">
      {#if probing}
        <span class="muted">{t("new.probing")}</span>
      {:else}
        <strong>{size === null ? t("new.unknown") : fmt.bytes(size)}</strong>
        {#if probe}
          <span class={probe.acceptRanges ? "ok" : "warn"}>
            · {probe.acceptRanges ? t("new.ranges") : t("new.noRanges")}
          </span>
        {/if}
      {/if}
    </div>

    <label for="nd-conn">{t("new.connections")}</label>
    <div class="row">
      <input id="nd-conn" type="range" min="1" max="32" bind:value={connections} />
      <span class="conn">{connections}</span>
    </div>

    <label for="nd-sum">{t("new.checksum")}</label>
    <input id="nd-sum" class="mono" bind:value={checksum} placeholder="sha256:… / md5:…" title={t("new.checksumHint")} />

    {#if pending?.source || pending?.referrer}
      <span class="lbl">{t("new.from")}</span>
      <div class="info ellipsis muted" title={pending.referrer ?? ""}>
        {pending.source ?? ""}
        {pending.referrer ? `· ${pending.referrer}` : ""}
      </div>
    {/if}

    {#if match}
      {@const pct = fmt.percent(match.downloaded, match.totalSize)}
      <div class="full matchbox" role="alert">
        <div class="matchtext">
          <span>{t("match.title")}</span>
          <strong class="ellipsis">{match.filename}</strong>
          <span class="muted">
            {pct === null ? fmt.bytes(match.downloaded) : `${pct.toFixed(1)}%`}
            · {fmt.bytes(match.downloaded)} / {fmt.bytes(match.totalSize)} — {t("match.hint")}
          </span>
        </div>
        <button type="button" class="primary" onclick={continueExisting} disabled={busy}>
          {t("match.resume")}
        </button>
      </div>
    {/if}
    {#if exists && !match}
      <div class="full warnbox" role="alert">
        {t("new.exists")}
        <label class="check"><input type="radio" bind:group={conflict} value="rename" />{t("new.rename")}</label>
        <label class="check"><input type="radio" bind:group={conflict} value="overwrite" />{t("new.overwrite")}</label>
      </div>
    {/if}
    {#if error}
      <div class="full errbox" role="alert">{error}</div>
    {/if}
    <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
  </form>

  {#snippet footer()}
    {#if more}
      <span class="muted more">+{more} {t("new.more")}</span>
    {/if}
    <button onclick={cancel}>{t("new.cancel")}</button>
    <button onclick={() => submit(false)} disabled={busy || !url.trim()}>{t("new.later")}</button>
    <button class="primary" onclick={() => submit(true)} disabled={busy || !url.trim()}>{t("new.start")}</button>
  {/snippet}
</Modal>

<style>
  .grid {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 10px 12px;
    align-items: center;
  }
  label,
  .lbl {
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input:not([type="range"]) {
    flex: 1;
  }
  .row input[type="range"] {
    flex: 1;
    accent-color: var(--accent);
  }
  .conn {
    width: 24px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .info {
    min-width: 0;
  }
  .ok {
    color: var(--ok);
  }
  .warn {
    color: var(--warn);
  }
  .full {
    grid-column: 1 / -1;
  }
  .warnbox,
  .errbox {
    padding: 8px 10px;
    border-radius: 6px;
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: center;
  }
  .warnbox {
    background: color-mix(in srgb, var(--warn) 14%, transparent);
  }
  .warnbox label {
    color: var(--text);
  }
  .matchbox {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 10px 12px;
    border-radius: 6px;
    background: var(--accent-soft);
  }
  .matchtext {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .errbox {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
  }
  .more {
    margin-right: auto;
    align-self: center;
  }
</style>
