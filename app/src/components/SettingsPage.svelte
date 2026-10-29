<script lang="ts">
  import { api, errorMessage } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t } from "../lib/i18n.svelte";
  import { app, saveSettings } from "../lib/store.svelte";
  import type { Settings, ToolStatus } from "../lib/types";

  let s = $state<Settings>(structuredClone($state.snapshot(app.settings!)));
  let limit = $state(fmt.rateInput(s.speedLimit));
  let error = $state<string | null>(null);
  let savedAt = $state<string | null>(null);

  // Every control saves itself, shortly after the last change.
  let first = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const snapshot = $state.snapshot(s);
    if (first) {
      first = false;
      return;
    }
    clearTimeout(timer);
    timer = setTimeout(() => void save(snapshot), 350);
  });

  async function save(next: Settings) {
    error = null;
    try {
      await saveSettings({
        ...next,
        proxy: next.proxy?.trim() || null,
        schedule: {
          enabled: next.schedule.enabled,
          start: next.schedule.start || null,
          stop: next.schedule.stop || null,
        },
      });
      savedAt = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function applyLimit() {
    s.speedLimit = fmt.parseRate(limit);
    limit = fmt.rateInput(s.speedLimit);
  }

  async function browse() {
    const p = await api.pickFolder(s.downloadDir);
    if (p) s.downloadDir = p;
  }

  let tools = $state<ToolStatus[]>([]);
  let updating = $state(false);
  let toolNote = $state<string | null>(null);
  const loadTools = async () => (tools = await api.toolStatus().catch(() => []));
  void loadTools();

  async function updateYtdlp() {
    updating = true;
    toolNote = null;
    try {
      const v = await api.updateYtdlp();
      toolNote = v ? `${t("set.updated")} ${v}` : t("set.upToDate");
      await loadTools();
    } catch (e) {
      toolNote = errorMessage(e);
    } finally {
      updating = false;
    }
  }

  function toolText(x: ToolStatus) {
    if (!x.path) return t("set.toolMissing");
    const where = x.managed ? t("set.toolManaged") : t("set.toolSystem");
    return x.version ? `${x.version} · ${where}` : where;
  }
</script>

<main>
  <div class="column">
    <header>
      <div class="grow">
        <h1>{t("set.title")}</h1>
        <p class="muted">{t("set.autosave")}</p>
      </div>
      {#if error}
        <span class="pill bad" role="alert">{error}</span>
      {:else if savedAt}
        <span class="muted" aria-live="polite">{t("set.saved")} <span class="num">{savedAt}</span></span>
      {/if}
    </header>

    <section>
      <div class="label"><h2>{t("set.general")}</h2><p>{t("set.generalHint")}</p></div>
      <div class="panel">
        <div class="item">
          <label for="s-lang">{t("set.language")}</label>
          <select id="s-lang" bind:value={s.language}>
            <option value="id">Bahasa Indonesia</option>
            <option value="en">English</option>
          </select>
        </div>
        <div class="item">
          <span id="s-theme">{t("set.theme")}</span>
          <div class="lines" role="radiogroup" aria-labelledby="s-theme">
            {#each [["system", "set.themeSystem"], ["light", "set.themeLight"], ["dark", "set.themeDark"]] as const as [v, key] (v)}
              <button role="radio" aria-checked={s.theme === v} onclick={() => (s.theme = v)}>{t(key)}</button>
            {/each}
          </div>
        </div>
        <label class="item">
          <span>{t("set.autostart")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.autostart} />
        </label>
        <label class="item">
          <span>{t("set.notifications")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.notifications} />
        </label>
      </div>
    </section>

    <section>
      <div class="label">
        <h2>{t("set.downloads")}</h2>
        <p>{t("set.downloadsHint")}</p>
      </div>
      <div class="panel">
        <div class="item wide">
          <label for="s-dir">{t("set.folder")}</label>
          <div class="field">
            <input id="s-dir" bind:value={s.downloadDir} />
            <button onclick={browse}>{t("new.browse")}</button>
          </div>
        </div>
        <label class="item">
          <span>{t("set.categoryFolders")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.categoryFolders} />
        </label>
        <div class="item">
          <label for="s-conn">{t("set.connections")}</label>
          <div class="field">
            <input id="s-conn" type="range" min="1" max="32" bind:value={s.connections} />
            <span class="num val">{s.connections}</span>
          </div>
        </div>
        <div class="item">
          <label for="s-max">{t("set.maxActive")}</label>
          <input id="s-max" class="num short" type="number" min="1" max="20" bind:value={s.maxActive} />
        </div>
        <div class="item">
          <label for="s-limit">{t("set.speedLimit")}<span class="hint">{t("detail.limitHint")}</span></label>
          <input id="s-limit" class="num" bind:value={limit} onchange={applyLimit} placeholder="—" />
        </div>
        <div class="item">
          <span id="s-dup">{t("set.duplicate")}</span>
          <div class="lines" role="radiogroup" aria-labelledby="s-dup">
            {#each [["rename", "set.dupRename"], ["overwrite", "set.dupOverwrite"], ["ask", "set.dupAsk"]] as const as [v, key] (v)}
              <button role="radio" aria-checked={s.duplicatePolicy === v} onclick={() => (s.duplicatePolicy = v)}>{t(key)}</button>
            {/each}
          </div>
        </div>
      </div>
    </section>

    <section>
      <div class="label"><h2>{t("set.browser")}</h2><p>{t("set.extension")}</p></div>
      <div class="panel">
        <label class="item">
          <span>{t("set.confirmNew")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.confirmNew} />
        </label>
        <label class="item">
          <span>{t("set.clipboard")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.clipboardMonitor} />
        </label>
      </div>
    </section>

    <section>
      <div class="label"><h2>{t("set.video")}</h2><p>{t("set.videoHint")}</p></div>
      <div class="panel">
        <div class="item wide">
          <span>{t("set.tools")}</span>
          <dl class="tools">
            {#each tools as x (x.tool)}
              <dt>{x.tool}</dt>
              <dd class="muted" title={x.path ?? ""}>{toolText(x)}</dd>
            {/each}
          </dl>
          <div class="field">
            <button onclick={updateYtdlp} disabled={updating}>{t("set.updateYtdlp")}</button>
            {#if toolNote}<span class="muted" aria-live="polite">{toolNote}</span>{/if}
          </div>
        </div>
        <label class="item">
          <span>{t("set.autoUpdate")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.video.autoUpdate} />
        </label>
        <div class="item wide">
          <label for="s-ytdlp">{t("set.ytdlpPath")}<span class="hint">{t("set.pathHint")}</span></label>
          <div class="field">
            <input id="s-ytdlp" class="mono" bind:value={s.video.ytdlpPath} onchange={loadTools} placeholder="—" />
          </div>
        </div>
        <div class="item wide">
          <label for="s-ffmpeg">{t("set.ffmpegPath")}<span class="hint">{t("set.pathHint")}</span></label>
          <div class="field">
            <input id="s-ffmpeg" class="mono" bind:value={s.video.ffmpegPath} onchange={loadTools} placeholder="—" />
          </div>
        </div>
      </div>
    </section>

    <section>
      <div class="label"><h2>{t("set.schedule")}</h2><p>{t("set.scheduleHint")}</p></div>
      <div class="panel">
        <label class="item">
          <span>{t("set.scheduleEnabled")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.schedule.enabled} />
        </label>
        <div class="item">
          <label for="s-start">{t("set.scheduleStart")}</label>
          <input id="s-start" class="num" type="time" bind:value={s.schedule.start} disabled={!s.schedule.enabled} />
        </div>
        <div class="item">
          <label for="s-stop">{t("set.scheduleStop")}</label>
          <input id="s-stop" class="num" type="time" bind:value={s.schedule.stop} disabled={!s.schedule.enabled} />
        </div>
        <div class="item">
          <label for="s-after">{t("set.afterQueue")}</label>
          <select id="s-after" bind:value={s.afterQueue}>
            <option value="nothing">{t("set.afterNothing")}</option>
            <option value="sleep">{t("set.afterSleep")}</option>
            <option value="shutdown">{t("set.afterShutdown")}</option>
          </select>
        </div>
      </div>
    </section>

    <section>
      <div class="label"><h2>{t("set.network")}</h2><p>{t("set.networkHint")}</p></div>
      <div class="panel">
        <div class="item wide">
          <label for="s-proxy">{t("set.proxy")}</label>
          <input id="s-proxy" class="mono" bind:value={s.proxy} placeholder="socks5://127.0.0.1:1080" />
        </div>
      </div>
    </section>

    <p class="muted version">Oxyd · {t("set.version")} <span class="num">{app.version}</span></p>
  </div>
</main>

<style>
  main {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 28px 32px 48px;
  }
  .column {
    max-width: 1100px;
    margin: 0 auto;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 16px;
    padding-bottom: 28px;
  }
  .grow {
    flex: 1 1 300px;
  }
  h1 {
    margin: 0;
    font-size: 26px;
    line-height: 32px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  header p {
    margin: 6px 0 0;
    font-size: 14px;
  }
  section {
    display: flex;
    flex-wrap: wrap;
    gap: 20px 48px;
    padding: 32px 0;
    border-top: 1px solid var(--border);
  }
  .label {
    flex: 1 1 260px;
    max-width: 340px;
  }
  .label h2 {
    margin: 0;
    font-size: 16px;
    line-height: 22px;
    font-weight: 600;
  }
  .label p {
    margin: 4px 0 0;
    color: var(--muted);
  }
  .panel {
    flex: 999 1 480px;
    min-width: 0;
    padding: 4px 20px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--card-border);
  }
  .item {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
    min-height: 56px;
    padding: 10px 0;
    border-top: 1px solid var(--rule);
    font-size: 14px;
  }
  .item:first-child {
    border-top: 0;
  }
  label.item {
    cursor: pointer;
  }
  .item > :first-child {
    flex: 1 1 200px;
    min-width: 0;
  }
  .wide {
    flex-direction: column;
    align-items: stretch;
  }
  .wide > :first-child {
    flex: none;
  }
  .hint {
    display: block;
    font-size: 12px;
    line-height: 18px;
    color: var(--muted);
  }
  .field {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .wide .field input,
  .wide > input {
    flex: 1;
    width: 100%;
  }
  .val {
    width: 24px;
    text-align: right;
  }
  .short {
    width: 72px;
    text-align: right;
  }
  #s-limit {
    width: 120px;
    text-align: right;
  }
  .tools {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 4px 16px;
    margin: 0;
  }
  .tools dt {
    font-family: var(--font-num);
    font-size: 13px;
  }
  .tools dd {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .version {
    padding-top: 32px;
    border-top: 1px solid var(--border);
    margin: 0;
  }
</style>
