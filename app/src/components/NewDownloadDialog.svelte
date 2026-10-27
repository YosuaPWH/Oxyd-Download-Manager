<script lang="ts">
  import { onMount, tick } from "svelte";
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

  /** The link the current `probe` belongs to. */
  let probedUrl = $state("");
  /** The link of the latest check, finished or not; only the latest one may update the form. */
  let checking = "";
  let seq = 0;
  let urlInput = $state<HTMLInputElement>();

  const size = $derived(probe?.size ?? pending?.size ?? null);
  const host = $derived.by(() => {
    try {
      return new URL(probe?.finalUrl ?? url.trim()).host;
    } catch {
      return "";
    }
  });
  // Browser downloads arrive checked; a typed link shows its details once it was checked.
  const ready = $derived(!!pending || (!!probe && probedUrl === url.trim()));

  async function check() {
    const target = url.trim();
    if (!target) return;
    const id = ++seq;
    checking = target;
    probing = true;
    error = null;
    if (!pending) match = null;
    try {
      const info = pending ? await api.probePending(pending.key) : await api.probe(target);
      if (id !== seq) return;
      if (!nameTouched && info.filename && (!filename || !pending?.filename)) {
        filename = info.filename;
      }
      const name = filename || info.filename;
      if (!match && name) {
        const d = await api.findUnfinished(name, info.size);
        if (id !== seq) return;
        match = d
          ? { id: d.id, filename: d.filename, downloaded: d.downloaded, totalSize: d.totalSize }
          : null;
      }
      probe = info;
      probedUrl = target;
    } catch (e) {
      if (id !== seq) return;
      error = errorMessage(e);
      probe = null;
    } finally {
      if (id === seq) probing = false;
    }
  }

  function onUrlInput(e: Event) {
    if ((e as InputEvent).inputType === "insertFromPaste") void check();
  }

  function onUrlChange() {
    if (url.trim() !== checking) void check();
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
    void (async () => {
      // A link copied just before pressing Add URL is filled in, selected so typing replaces it.
      if (!url && !pending) {
        const link = await api.clipboardLink().catch(() => null);
        if (!link || url.trim()) return;
        url = link;
        await tick();
        urlInput?.select();
      }
      if (url) void check();
    })();
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

<Modal title={t("new.title")} onclose={cancel} width={540}>
  <form class="stack" onsubmit={(e) => (e.preventDefault(), ready ? submit(true) : check())}>
    <div>
      <label for="nd-url">{t("new.url")}</label>
      <div class="row">
        <input
          id="nd-url"
          class="mono"
          bind:value={url}
          bind:this={urlInput}
          readonly={!!pending}
          placeholder="https://"
          oninput={onUrlInput}
          onchange={onUrlChange}
        />
        {#if !pending}
          <button type="button" class:primary={!ready} onclick={check} disabled={probing || !url.trim()}>
            {t("new.check")}
          </button>
        {/if}
      </div>
      {#if !ready}
        <p class="hint muted" aria-live="polite">{probing ? t("new.probing") : error ? "" : t("new.hint")}</p>
      {/if}
    </div>

    {#if ready}
      <div class="facts">
        <span class="num big">{probing ? "…" : size === null ? t("new.unknown") : fmt.bytes(size)}</span>
        <span class="muted r ellipsis">{host}</span>
        {#if probe}
          <span class:muted={probe.acceptRanges} class:warn={!probe.acceptRanges}>
            {probe.acceptRanges ? t("new.ranges") : t("new.noRanges")}
          </span>
        {:else}
          <span></span>
        {/if}
        {#if pending?.source}<span class="muted r">{pending.source}</span>{/if}
      </div>

      <div>
        <label for="nd-name">{t("new.filename")}</label>
        <input id="nd-name" bind:value={filename} oninput={() => (nameTouched = true)} />
      </div>

      <div>
        <label for="nd-dir">{t("new.folder")}</label>
        <div class="row">
          <input id="nd-dir" bind:value={dir} oninput={() => (dirTouched = true)} />
          <button type="button" onclick={browse}>{t("new.browse")}</button>
        </div>
      </div>

      <div class="two">
        <div>
          <label for="nd-conn">{t("new.connections")}</label>
          <div class="row range">
            <input id="nd-conn" type="range" min="1" max="32" bind:value={connections} />
            <span class="num conn">{connections}</span>
          </div>
        </div>
        <div>
          <label for="nd-sum">{t("new.checksum")}</label>
          <input id="nd-sum" class="mono" bind:value={checksum} placeholder="sha256:…" title={t("new.checksumHint")} />
        </div>
      </div>

      {#if match}
        {@const pct = fmt.percent(match.downloaded, match.totalSize)}
        <div class="box" role="alert">
          <div class="matchtext">
            <span>{t("match.title")}</span>
            <strong class="ellipsis">{match.filename}</strong>
            <span class="muted">
              <span class="num">{pct === null ? fmt.bytes(match.downloaded) : `${pct.toFixed(1)}%`}</span>
              · <span class="num">{fmt.bytes(match.downloaded)} / {fmt.bytes(match.totalSize)}</span> — {t("match.hint")}
            </span>
          </div>
          <button type="button" class="primary" onclick={continueExisting} disabled={busy}>
            {t("match.resume")}
          </button>
        </div>
      {/if}
      {#if exists && !match}
        <div class="box warnbox" role="alert">
          <span>{t("new.exists")}</span>
          <div class="lines" role="radiogroup" aria-label={t("new.exists")}>
            <button type="button" role="radio" aria-checked={conflict === "rename"} onclick={() => (conflict = "rename")}>{t("new.rename")}</button>
            <button type="button" role="radio" aria-checked={conflict === "overwrite"} onclick={() => (conflict = "overwrite")}>{t("new.overwrite")}</button>
          </div>
        </div>
      {/if}
    {/if}
    {#if error}
      <div class="errbox" role="alert">{error}</div>
    {/if}
    <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
  </form>

  {#snippet footer()}
    {#if more}
      <span class="muted more">+<span class="num">{more}</span> {t("new.more")}</span>
    {/if}
    <button onclick={cancel}>{t("new.cancel")}</button>
    <button onclick={() => submit(false)} disabled={busy || !ready}>{t("new.later")}</button>
    <button class="primary" onclick={() => submit(true)} disabled={busy || !ready}>{t("new.start")}</button>
  {/snippet}
</Modal>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  label {
    display: block;
    margin-bottom: 6px;
    font-size: 12px;
    line-height: 16px;
    color: var(--muted);
  }
  .stack > div > input,
  .two input:not([type="range"]) {
    width: 100%;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input:not([type="range"]) {
    flex: 1;
  }
  .range {
    height: 34px;
  }
  .range input {
    flex: 1;
  }
  .conn {
    width: 22px;
    text-align: right;
  }
  .hint {
    margin: 8px 0 0;
  }
  .facts {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 2px 16px;
    align-items: baseline;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--raised);
  }
  .big {
    font-size: 20px;
    line-height: 26px;
  }
  .r {
    text-align: right;
  }
  .warn {
    color: var(--warning);
  }
  .two {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 16px;
  }
  .box {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--raised);
  }
  .matchtext {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .warnbox {
    justify-content: space-between;
  }
  .errbox {
    padding: 10px 14px;
    border-radius: var(--radius);
    background: var(--neg-bg);
    color: var(--negative);
  }
  .more {
    margin-right: auto;
    align-self: center;
  }
</style>
