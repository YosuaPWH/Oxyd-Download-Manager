<script lang="ts">
  import { api, errorMessage } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { app, saveSettings } from "../lib/store.svelte";
  import type { Settings } from "../lib/types";
  import Modal from "./Modal.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let s = $state<Settings>(structuredClone($state.snapshot(app.settings!)));
  let limit = $state(fmt.rateInput(s.speedLimit));
  let error = $state<string | null>(null);
  let tab = $state<"general" | "downloads" | "browser" | "schedule" | "network">("general");

  async function browse() {
    const p = await api.pickFolder(s.downloadDir);
    if (p) s.downloadDir = p;
  }

  async function save() {
    error = null;
    try {
      await saveSettings({
        ...s,
        speedLimit: fmt.parseRate(limit),
        proxy: s.proxy?.trim() || null,
        schedule: {
          enabled: s.schedule.enabled,
          start: s.schedule.start || null,
          stop: s.schedule.stop || null,
        },
      });
      onclose();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  const tabs = [
    ["general", "set.general"],
    ["downloads", "set.downloads"],
    ["browser", "set.browser"],
    ["schedule", "set.schedule"],
    ["network", "set.network"],
  ] as const;
</script>

<Modal title={t("set.title")} {onclose} width={640}>
  <div class="layout">
    <div class="tabs" role="tablist">
      {#each tabs as [id, key] (id)}
        <button role="tab" aria-selected={tab === id} class:on={tab === id} onclick={() => (tab = id)}>{t(key)}</button>
      {/each}
    </div>
    <div class="panel" role="tabpanel">
      {#if tab === "general"}
        <label for="s-lang">{t("set.language")}</label>
        <select id="s-lang" bind:value={s.language}>
          <option value="id">Bahasa Indonesia</option>
          <option value="en">English</option>
        </select>
        <label for="s-theme">{t("set.theme")}</label>
        <select id="s-theme" bind:value={s.theme}>
          <option value="system">{t("set.themeSystem")}</option>
          <option value="light">{t("set.themeLight")}</option>
          <option value="dark">{t("set.themeDark")}</option>
        </select>
        <label class="check full"><input type="checkbox" bind:checked={s.autostart} />{t("set.autostart")}</label>
        <label class="check full"><input type="checkbox" bind:checked={s.notifications} />{t("set.notifications")}</label>
      {:else if tab === "downloads"}
        <label for="s-dir">{t("set.folder")}</label>
        <div class="row">
          <input id="s-dir" bind:value={s.downloadDir} />
          <button onclick={browse}>{t("new.browse")}</button>
        </div>
        <label class="check full"><input type="checkbox" bind:checked={s.categoryFolders} />{t("set.categoryFolders")}</label>
        <label for="s-conn">{t("set.connections")}</label>
        <div class="row">
          <input id="s-conn" type="range" min="1" max="32" bind:value={s.connections} />
          <span class="val">{s.connections}</span>
        </div>
        <label for="s-max">{t("set.maxActive")}</label>
        <input id="s-max" type="number" min="1" max="20" bind:value={s.maxActive} />
        <label for="s-limit">{t("set.speedLimit")}</label>
        <input id="s-limit" bind:value={limit} placeholder={t("detail.limitHint")} />
        <label for="s-dup">{t("set.duplicate")}</label>
        <select id="s-dup" bind:value={s.duplicatePolicy}>
          <option value="rename">{t("set.dupRename")}</option>
          <option value="overwrite">{t("set.dupOverwrite")}</option>
          <option value="ask">{t("set.dupAsk")}</option>
        </select>
      {:else if tab === "browser"}
        <p class="full muted">{t("set.extension")}</p>
        <label class="check full"><input type="checkbox" bind:checked={s.confirmNew} />{t("set.confirmNew")}</label>
        <label class="check full"><input type="checkbox" bind:checked={s.clipboardMonitor} />{t("set.clipboard")}</label>
      {:else if tab === "schedule"}
        <label class="check full"><input type="checkbox" bind:checked={s.schedule.enabled} />{t("set.scheduleEnabled")}</label>
        <label for="s-start">{t("set.scheduleStart")}</label>
        <input id="s-start" type="time" bind:value={s.schedule.start} disabled={!s.schedule.enabled} />
        <label for="s-stop">{t("set.scheduleStop")}</label>
        <input id="s-stop" type="time" bind:value={s.schedule.stop} disabled={!s.schedule.enabled} />
        <label for="s-after">{t("set.afterQueue")}</label>
        <select id="s-after" bind:value={s.afterQueue}>
          <option value="nothing">{t("set.afterNothing")}</option>
          <option value="sleep">{t("set.afterSleep")}</option>
          <option value="shutdown">{t("set.afterShutdown")}</option>
        </select>
      {:else}
        <label class="full" for="s-proxy">{t("set.proxy")}</label>
        <input class="full mono" id="s-proxy" bind:value={s.proxy} placeholder="socks5://127.0.0.1:1080" />
      {/if}
      {#if error}<div class="full err" role="alert">{error}</div>{/if}
    </div>
  </div>
  {#snippet footer()}
    <span class="muted ver">Oxyd {app.version}</span>
    <button onclick={onclose}>{t("set.cancel")}</button>
    <button class="primary" onclick={save}>{t("set.save")}</button>
  {/snippet}
</Modal>

<style>
  .layout {
    display: flex;
    gap: 16px;
    min-height: 300px;
  }
  .tabs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 160px;
    flex-shrink: 0;
  }
  .tabs button {
    border: none;
    background: transparent;
    justify-content: flex-start;
  }
  .tabs button.on {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .panel {
    flex: 1;
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 10px 12px;
    align-items: center;
    align-content: start;
  }
  .panel > label:not(.check) {
    color: var(--muted);
  }
  .full {
    grid-column: 1 / -1;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input {
    flex: 1;
    accent-color: var(--accent);
  }
  .val {
    width: 24px;
    text-align: right;
  }
  .err {
    color: var(--danger);
  }
  .ver {
    margin-right: auto;
    align-self: center;
  }
</style>
