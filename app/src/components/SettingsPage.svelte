<script lang="ts">
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { api, errorMessage } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t, type Key } from "../lib/i18n.svelte";
  import { app, saveSettings } from "../lib/store.svelte";
  import mark from "../../src-tauri/icons/source.svg";
  import { ACCENTS, type Settings, type ToolStatus } from "../lib/types";

  const initial = $state.snapshot(app.settings!);
  let s = $state<Settings>(structuredClone(initial));
  let limit = $state(fmt.rateInput(s.speedLimit));
  let error = $state<string | null>(null);
  let savedAt = $state<string | null>(null);

  // Debounce typing and serialize writes. Send only edited fields because another window
  // can change the global speed limit while this form is open.
  let previous = initial;
  let pending: Partial<Settings> = {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  let saving = Promise.resolve();

  function collectChanges() {
    const snapshot = $state.snapshot(s);
    const changed = Object.fromEntries(
      Object.entries(snapshot).filter(([k, value]) =>
        JSON.stringify(value) !== JSON.stringify(previous[k as keyof Settings]),
      ),
    );
    previous = snapshot;
    Object.assign(pending, changed);
  }

  function flush() {
    clearTimeout(timer);
    const patch = pending;
    pending = {};
    if (Object.keys(patch).length) saving = saving.then(() => save(patch));
    return saving;
  }

  $effect(() => {
    collectChanges();
    clearTimeout(timer);
    if (Object.keys(pending).length) timer = setTimeout(() => void flush(), 350);
  });

  onMount(() => {
    if (!isTauri()) return;
    const unlisten = getCurrentWindow().onCloseRequested(async () => {
      collectChanges();
      await flush();
    });
    return () => void unlisten.then((stop) => stop()).catch(() => {});
  });

  async function save(next: Partial<Settings>) {
    error = null;
    try {
      if ("proxy" in next) next.proxy = next.proxy?.trim() || null;
      if (next.schedule) {
        const { start, stop } = next.schedule;
        next.schedule = { ...next.schedule, start: start || null, stop: stop || null };
      }
      await saveSettings(next);
      savedAt = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      if (next.video) void loadTools();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  // The swatches show each accent as it looks on the current theme's buttons.
  const SWATCH: Record<(typeof ACCENTS)[number], string> = {
    oxide: "#2aa896",
    copper: "#c8713a",
    azure: "#3f80e6",
    violet: "#7462ea",
    rose: "#cf4169",
    graphite: "#8a9391",
  };
  const DAYS = [0, 1, 2, 3, 4, 5, 6];

  function toggleDay(d: number) {
    const days = s.schedule.days;
    s.schedule.days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d].sort();
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
  const ytdlp = $derived(tools.find((tool) => tool.tool === "yt-dlp"));
  const installing = $derived(ytdlp?.path == null);
  let updating = $state(false);
  let toolNote = $state<string | null>(null);
  const loadTools = async () => (tools = await api.toolStatus().catch(() => []));
  void loadTools();

  async function updateYtdlp() {
    updating = true;
    toolNote = null;
    try {
      await flush();
      const wasMissing = installing;
      const v = await api.updateYtdlp();
      toolNote = v ? `${t(wasMissing ? "set.installedYtdlp" : "set.updated")} ${v}` : t("set.upToDate");
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
        <label class="item">
          <span>{t("set.progressWindow")}<span class="hint">{t("set.progressWindowHint")}</span></span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.progressWindow} />
        </label>
        <label class="item">
          <span>{t("set.sound")}</span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.sound} />
        </label>
      </div>
    </section>

    <section>
      <div class="label"><h2>{t("set.appearance")}</h2><p>{t("set.appearanceHint")}</p></div>
      <div class="panel">
        <div class="item">
          <span id="s-accent">{t("set.accent")}</span>
          <div class="swatches" role="radiogroup" aria-labelledby="s-accent">
            {#each ACCENTS as a (a)}
              <button
                class="swatch"
                role="radio"
                aria-checked={s.accent === a}
                aria-label={t(`accent.${a}` as Key)}
                title={t(`accent.${a}` as Key)}
                style:--c={SWATCH[a]}
                onclick={() => (s.accent = a)}
              ></button>
            {/each}
          </div>
        </div>
        <div class="item">
          <span id="s-density">{t("set.density")}</span>
          <div class="lines" role="radiogroup" aria-labelledby="s-density">
            {#each [["comfortable", "set.densityComfortable"], ["compact", "set.densityCompact"]] as const as [v, key] (v)}
              <button role="radio" aria-checked={s.density === v} onclick={() => (s.density = v)}>{t(key)}</button>
            {/each}
          </div>
        </div>
        <label class="item">
          <span>{t("set.glass")}<span class="hint">{t("set.glassHint")}</span></span>
          <input type="checkbox" class="switch" role="switch" bind:checked={s.glass} />
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
            {#if ytdlp && (installing || ytdlp.managed)}
              <button onclick={updateYtdlp} disabled={updating}>
                {t(updating ? "set.fetchingYtdlp" : installing ? "set.installYtdlp" : "set.updateYtdlp")}
              </button>
            {/if}
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
            <input id="s-ytdlp" class="mono" bind:value={s.video.ytdlpPath} placeholder="—" />
          </div>
        </div>
        <div class="item wide">
          <label for="s-ffmpeg">{t("set.ffmpegPath")}<span class="hint">{t("set.pathHint")}</span></label>
          <div class="field">
            <input id="s-ffmpeg" class="mono" bind:value={s.video.ffmpegPath} placeholder="—" />
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
          <span id="s-days">{t("set.scheduleDays")}<span class="hint">{t("set.scheduleDaysHint")}</span></span>
          <div class="days" role="group" aria-labelledby="s-days">
            {#each DAYS as d (d)}
              <button aria-pressed={s.schedule.days.includes(d)} disabled={!s.schedule.enabled} onclick={() => toggleDay(d)}>
                {t(`day.${d}` as Key)}
              </button>
            {/each}
          </div>
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

    <section aria-labelledby="about">
      <div class="label"><h2 id="about">{t("set.about")}</h2></div>
      <div class="panel">
        <div class="item">
          <div class="brand">
            <img src={mark} alt="" width="36" height="36" />
            <div>
              <strong>Oxyd Download Manager</strong>
              <span class="hint">{t("set.version")} <span class="num">{app.version}</span> · MIT</span>
            </div>
          </div>
          <button onclick={() => api.openProject().catch((e) => (error = errorMessage(e)))}>
            {t("set.sourceCode")}
          </button>
        </div>
      </div>
    </section>
  </div>
</main>

<style>
  main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    height: 100%;
    overflow-y: auto;
    padding: 20px 24px 40px;
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
    padding-bottom: 20px;
  }
  .grow {
    flex: 1 1 300px;
  }
  h1 {
    margin: 0;
    font-size: 18px;
    line-height: 24px;
    font-weight: 600;
    letter-spacing: -0.015em;
  }
  header p {
    margin: 2px 0 0;
    font-size: 12.5px;
  }
  section {
    display: flex;
    flex-wrap: wrap;
    gap: 16px 48px;
    padding: 24px 0;
    border-top: 1px solid var(--border);
  }
  .label {
    flex: 1 1 260px;
    max-width: 340px;
  }
  .label h2 {
    margin: 0;
    font-size: 14px;
    line-height: 20px;
    font-weight: 600;
  }
  .label p {
    margin: 4px 0 0;
    color: var(--muted);
  }
  .panel {
    flex: 999 1 480px;
    min-width: 0;
    padding: 2px 16px;
    border-radius: 10px;
    background: var(--raised);
    box-shadow: var(--sheet-shadow);
  }
  .item {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
    min-height: 48px;
    padding: 8px 0;
    border-top: 1px solid var(--rule);
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
  .swatches {
    display: flex;
    gap: 10px;
  }
  .swatch {
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--c);
  }
  .swatch:hover:not(:disabled) {
    background: var(--c);
    transform: scale(1.1);
  }
  .swatch[aria-checked="true"] {
    box-shadow: 0 0 0 2px var(--raised), 0 0 0 4px var(--text);
  }
  .days {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .days button {
    min-width: 38px;
    padding: 0 6px;
    font-size: 12px;
    color: var(--muted);
    border-color: var(--border);
  }
  .days button[aria-pressed="true"] {
    color: var(--accent);
    font-weight: 600;
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .brand img {
    flex: none;
  }
</style>
