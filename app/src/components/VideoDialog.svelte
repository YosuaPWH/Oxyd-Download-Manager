<script lang="ts">
  import { untrack } from "svelte";
  import { api, errorMessage } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { app } from "../lib/store.svelte";
  import type { PendingAdd, VideoChoice, VideoProbe } from "../lib/types";
  import Modal from "./Modal.svelte";

  let {
    pending = null,
    probe,
    onclose,
  }: { pending?: PendingAdd | null; probe: VideoProbe; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  const preset = pending?.video?.choice;
  // svelte-ignore state_referenced_locally
  let choiceId = $state(
    probe.choices.find((c) => c.id === preset)?.id ??
      probe.choices.find((c) => c.kind === "video")?.id ??
      probe.choices[0]?.id ??
      "",
  );
  let filename = $state("");
  let dir = $state("");
  let nameTouched = false;
  let dirTouched = false;
  let busy = $state(false);
  let error = $state<string | null>(null);

  const choice = $derived(probe.choices.find((c) => c.id === choiceId) ?? null);
  const videos = $derived(probe.choices.filter((c) => c.kind === "video"));
  const audios = $derived(probe.choices.filter((c) => c.kind === "audio"));

  // The name follows the quality's container until the user edits it.
  $effect(() => {
    const c = choice;
    if (!c) return;
    if (!nameTouched) {
      void api.videoFileName(probe.title, c.container).then((n) => {
        if (!nameTouched) filename = n;
      });
    } else {
      // Keep the typed name, with the right extension.
      const typed = untrack(() => filename);
      filename = typed.replace(/\.[a-z0-9]{2,4}$/i, "") + `.${c.container}`;
    }
  });

  $effect(() => {
    const name = filename;
    if (!dirTouched && name) {
      void api.suggestDir(name).then((d) => {
        if (!dirTouched) dir = d;
      });
    }
  });

  async function browse() {
    const picked = await api.pickFolder(dir || null);
    if (picked) {
      dir = picked;
      dirTouched = true;
    }
  }

  async function submit(start: boolean) {
    if (!choice) return;
    busy = true;
    error = null;
    const options = { choice: choice.id, filename: filename.trim() || null, dir: dir || null, start };
    try {
      if (pending) await api.confirmVideo(pending.key, options);
      else await api.addVideo(probe.pageUrl, options);
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

  function size(c: VideoChoice) {
    return c.size === null ? "" : `${t("video.about")} ${fmt.bytes(c.size)}`;
  }

  const more = $derived(Math.max(0, app.pending.length - 1));
  const host = $derived.by(() => {
    try {
      return new URL(probe.pageUrl).host;
    } catch {
      return "";
    }
  });
</script>

<Modal title={t("video.title")} onclose={cancel} width={560}>
  <form class="stack" onsubmit={(e) => (e.preventDefault(), submit(true))}>
    <div class="facts">
      <strong class="name">{probe.title}</strong>
      <span class="muted">
        {probe.site ?? host}{#if probe.uploader}{" · "}{probe.uploader}{/if}{#if probe.duration}{" · "}<span class="num"
            >{fmt.duration(Math.round(probe.duration))}</span
          >{/if}
      </span>
    </div>

    {#if probe.live}
      <div class="errbox" role="alert">{t("video.live")}</div>
    {:else if !probe.choices.length}
      <div class="errbox" role="alert">{t("video.none")}</div>
    {:else}
      <div role="radiogroup" aria-label={t("video.quality")} class="choices">
        {#each [[t("video.videoGroup"), videos], [t("video.audioGroup"), audios]] as const as [label, list] (label)}
          {#if list.length}
            <div class="group">{label}</div>
            {#each list as c (c.id)}
              <button
                type="button"
                role="radio"
                class="choice"
                aria-checked={choiceId === c.id}
                onclick={() => (choiceId = c.id)}
              >
                <span class="num q">{c.label}</span>
                <span class="muted grow">{c.detail}</span>
                <span class="muted num">{size(c)}</span>
              </button>
            {/each}
          {/if}
        {/each}
      </div>

      <div>
        <label for="vd-name">{t("new.filename")}</label>
        <input id="vd-name" bind:value={filename} oninput={() => (nameTouched = true)} />
      </div>
      <div>
        <label for="vd-dir">{t("new.folder")}</label>
        <div class="row">
          <input id="vd-dir" bind:value={dir} oninput={() => (dirTouched = true)} />
          <button type="button" onclick={browse}>{t("new.browse")}</button>
        </div>
      </div>
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
    <button onclick={() => submit(false)} disabled={busy || !choice}>{t("new.later")}</button>
    <button class="primary" onclick={() => submit(true)} disabled={busy || !choice}>{t("new.start")}</button>
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
  .stack > div > input {
    width: 100%;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input {
    flex: 1;
  }
  .facts {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--raised);
  }
  .name {
    overflow-wrap: anywhere;
  }
  .choices {
    display: flex;
    flex-direction: column;
    max-height: 300px;
    overflow-y: auto;
  }
  .group {
    margin: 8px 0 4px;
    font-size: 12px;
    color: var(--muted);
  }
  .group:first-child {
    margin-top: 0;
  }
  .choice {
    display: flex;
    align-items: baseline;
    gap: 12px;
    width: 100%;
    padding: 8px 12px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    text-align: left;
  }
  .choice[aria-checked="true"] {
    border-color: var(--accent);
    background: var(--raised);
  }
  .q {
    min-width: 72px;
  }
  .grow {
    flex: 1;
    min-width: 0;
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
