<script lang="ts">
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { api, errorMessage, onEngineEvent, type BrowserStatus } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t, type Key } from "../lib/i18n.svelte";
  import { app, saveSettings } from "../lib/store.svelte";
  import mark from "../../src-tauri/icons/source.svg";
  import { ACCENTS, type Category, type Queue, type Settings, type ToolStatus } from "../lib/types";

  import Icon from "./Icon.svelte";
  import QueueDialog from "./QueueDialog.svelte";

  const basicCapture = () => ({
    enabled: true, minSize: 1024 * 1024, extensions: [], mimeTypes: [],
    excludedDomains: [], videoButton: true, minVideoDuration: 0,
  });
  function readList(text: string): string[] {
    return [...new Set(text.toLowerCase().split(/[\s,;]+/).filter(Boolean))];
  }

  const SECTIONS = [
    { id: "general", title: "set.general", icon: "settings", keys: ["set.language", "set.autostart", "set.clipboard"] },
    { id: "appearance", title: "set.appearance", icon: "image", keys: ["set.theme", "set.accent", "set.density", "set.glass"] },
    { id: "downloads", title: "set.downloads", icon: "down", keys: ["set.folder", "set.duplicate", "set.progressWindow"] },
    { id: "connection", title: "set.connection", icon: "globe", keys: ["set.connections", "set.connectTimeout", "set.httpVersion", "set.readTimeout", "set.userAgent"] },
    { id: "proxy", title: "set.proxyTitle", icon: "link", keys: ["set.proxyAddress", "set.directConnection", "set.proxyBypass"] },
    { id: "queues", title: "set.speedQueues", icon: "gauge", keys: ["set.speedLimit", "queue.title", "set.maxActive"] },
    { id: "categories", title: "nav.categories", icon: "layers", keys: ["set.categoryFolders"] },
    { id: "browser", title: "set.browser", icon: "panel", keys: ["set.capture", "set.confirmNew", "set.repairBridge"] },
    { id: "video", title: "set.video", icon: "video", keys: ["set.tools", "set.ytdlpPath", "set.ffmpegPath", "set.installTools", "set.subtitles", "set.sponsorblock"] },
    { id: "schedule", title: "set.schedule", icon: "calendar", keys: ["set.scheduleStart", "set.scheduleStop", "set.afterQueue"] },
    { id: "notifications", title: "set.notificationsSounds", icon: "music", keys: ["set.notifications", "set.sound"] },
    { id: "security", title: "set.security", icon: "alert", keys: ["set.scanDownloads", "set.scanExtensions"] },
    { id: "advanced", title: "set.advanced", icon: "wrench", keys: ["set.backup", "set.import", "set.export", "set.dataFolder", "set.downloadList", "set.diagnostics"] },
    { id: "shortcuts", title: "set.shortcuts", icon: "all", keys: ["tb.add", "tb.batchHint", "search.placeholder"] },
    { id: "about", title: "set.about", icon: "other", keys: ["set.version", "set.sourceCode", "set.releases"] },
  ] as const satisfies readonly { id: string; title: Key; icon: string; keys: readonly Key[] }[];

  type Section = (typeof SECTIONS)[number]["id"];
  let selected = $state<Section>("general");
  let search = $state("");
  let searchInput: HTMLInputElement;
  let content: HTMLElement;
  const matches = $derived.by(() => {
    const words = search.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
    return SECTIONS.filter(({ title, keys }) => {
      const text = [title, ...keys].map((key) => t(key)).join(" ").toLocaleLowerCase();
      return words.every((word) => text.includes(word));
    });
  });
  const section = $derived(matches.find(({ id }) => id === selected) ?? matches[0]);

  $effect(() => {
    if (section) content?.scrollTo({ top: 0 });
  });

  function keydown(event: KeyboardEvent) {
    if (queueEdit) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      searchInput.focus();
      searchInput.select();
    } else if (event.key === "Escape" && document.activeElement === searchInput && search) {
      event.preventDefault();
      search = "";
    }
  }

  const SHORTCUTS: readonly [Key, string][] = [
    ["tb.add", "Ctrl+N"], ["tb.batchHint", "Ctrl+B"], ["grab.title", "Ctrl+G"],
    ["search.placeholder", "Ctrl+F"], ["insp.title", "Ctrl+I"], ["tb.settings", "Ctrl+,"],
    ["set.pasteLinks", "Ctrl+V"], ["tb.removeSel", "Delete"], ["set.selectAll", "Ctrl+A"],
    ["set.toggleDownload", "Enter / Space"], ["set.dismiss", "Esc"],
  ];

  const CATEGORIES: readonly Category[] = ["video", "music", "image", "document", "archive", "program", "other"];

  const initial = $state.snapshot(app.settings!);
  let s = $state<Settings>(structuredClone(initial));
  let limit = $state(fmt.rateInput(initial.speedLimit));
  let error = $state<string | null>(null);
  let savedAt = $state<string | null>(null);
  let actionNote = $state<string | null>(null);
  let fileBusy = $state(false);
  let queueEdit = $state<Queue | null>(null);
  let queues = $state<Queue[]>([]);

  async function action(task: () => Promise<unknown>) {
    error = null;
    actionNote = null;
    try {
      await task();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function transferSettings(kind: "export" | "import") {
    fileBusy = true;
    try {
      await action(async () => {
        collectChanges();
        await flush();
        if (kind === "export") {
          if (await api.exportSettings()) actionNote = t("set.exportDone");
        } else {
          const imported = await api.importSettings();
          if (!imported) return;
          previous = imported;
          s = structuredClone(imported);
          limit = fmt.rateInput(s.speedLimit);
          actionNote = t("set.importDone");
          await loadTools();
          await refreshQueues();
        }
      });
    } finally {
      fileBusy = false;
    }
  }

  async function transferList(kind: "import" | "export" | "diagnostics") {
    fileBusy = true;
    try {
      await action(async () => {
        if (kind === "import") {
          const count = await api.importDownloads();
          if (count !== null) actionNote = `${t("set.listImported")} · ${count}`;
        } else {
          const saved = kind === "export" ? await api.exportDownloads() : await api.exportDiagnostics();
          if (saved) actionNote = t(kind === "export" ? "set.listExported" : "set.diagnosticsSaved");
        }
      });
    } finally {
      fileBusy = false;
    }
  }

  async function refreshQueues() {
    queues = await api.listQueues();
  }

  async function editQueue(queue: Queue) {
    await flush();
    queueEdit = queue;
  }

  async function closeQueue() {
    queueEdit = null;
    await action(async () => {
      await refreshQueues();
      const current = await api.getSettings();
      previous = { ...previous, maxActive: current.maxActive, schedule: current.schedule };
      s.maxActive = current.maxActive;
      s.schedule = current.schedule;
    });
  }

  let browser = $state<BrowserStatus | null>(null);
  let browserBusy = $state(false);

  async function refreshBrowser() {
    browser = await api.browserStatus();
  }

  $effect(() => {
    if (section?.id !== "browser") return;
    void refreshBrowser().catch(() => {});
    const timer = setInterval(() => void refreshBrowser().catch(() => {}), 3000);
    return () => clearInterval(timer);
  });

  async function repairBrowser() {
    browserBusy = true;
    await action(async () => {
      await api.repairBrowser();
      await refreshBrowser();
      actionNote = t("set.bridgeRepaired");
    });
    browserBusy = false;
  }

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
        value !== undefined && JSON.stringify(value) !== JSON.stringify(previous[k as keyof Settings]),
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
    void action(refreshQueues);
    const listeners = [
      onEngineEvent((event) => {
        if (event.type === "queues") queues = event.queues;
      }),
    ];
    if (isTauri()) {
      listeners.push(getCurrentWindow().onCloseRequested(async () => {
        collectChanges();
        await flush();
      }));
    }
    return () => {
      for (const listener of listeners) void listener.then((stop) => stop()).catch(() => {});
    };
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
      if (next.schedule || next.maxActive !== undefined) void refreshQueues().catch(() => {});
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
    await action(async () => {
      const path = await api.pickFolder(s.downloadDir);
      if (path) s.downloadDir = path;
    });
  }

  async function browseCategory(category: Category) {
    await action(async () => {
      const path = await api.pickFolder(s.categoryPaths[category] ?? s.downloadDir);
      if (path) s.categoryPaths[category] = path;
    });
  }

  let tools = $state<ToolStatus[]>([]);
  const ytdlp = $derived(tools.find((tool) => tool.tool === "yt-dlp"));
  const installing = $derived(ytdlp?.path == null);
  let updating = $state<"yt-dlp" | "all" | null>(null);
  let toolNote = $state<string | null>(null);
  const loadTools = async () => (tools = await api.toolStatus().catch(() => []));
  void loadTools();

  async function updateYtdlp() {
    updating = "yt-dlp";
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
      updating = null;
    }
  }

  async function installTools() {
    updating = "all";
    toolNote = null;
    try {
      await flush();
      tools = await api.ensureVideoTools();
      toolNote = t("set.toolsReady");
    } catch (e) {
      toolNote = errorMessage(e);
      await loadTools();
    } finally {
      updating = null;
    }
  }

  function toolText(x: ToolStatus) {
    if (!x.path) return t("set.toolMissing");
    const where = x.managed ? t("set.toolManaged") : t("set.toolSystem");
    return x.version ? `${x.version} · ${where}` : where;
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="settings-shell">
  <aside class="navigation">
    <div class="identity">
      <img src={mark} alt="" width="30" height="30" />
      <span>{t("set.title")}</span>
    </div>

    <label class="search">
      <Icon name="search" size={15} />
      <input type="search" bind:this={searchInput} bind:value={search} placeholder={t("set.search")} aria-label={t("set.search")} />
    </label>

    <nav aria-label={t("set.sections")}>
      {#each matches as item (item.id)}
        <button type="button" class:separate={!search && (item.id === "browser" || item.id === "notifications")} aria-current={item.id === section?.id ? "page" : undefined} onclick={() => (selected = item.id)}>
          <Icon name={item.icon} />
          <span>{t(item.title)}</span>
        </button>
      {/each}
    </nav>
  </aside>

  <main bind:this={content} aria-labelledby="settings-heading">
    <div class="column">
      <header>
        <div>
          <h1 id="settings-heading">{section ? t(section.title) : t("set.noResults")}</h1>
          {#if section}<p class="muted" aria-live="polite">
            {actionNote ?? (savedAt ? `${t("set.saved")} ${savedAt}` : t("set.autosave"))}
          </p>{/if}
        </div>
      </header>

      {#if error}
        <p class="feedback" role="alert">{error}</p>
      {/if}

      {#if section?.id === "general"}
        {@render group(t("set.language"), language)}
        {@render group(t("set.startup"), startup)}
      {:else if section?.id === "appearance"}
        {@render group(t("set.theme"), theme)}
        {@render group(t("set.accent"), accents)}
        {@render group(t("set.display"), display)}
      {:else if section?.id === "downloads"}
        {@render group(t("set.folder"), folder)}
        {@render group(t("set.downloadBehavior"), behavior)}
      {:else if section?.id === "connection"}
        {@render group(null, connection)}
      {:else if section?.id === "proxy"}
        {@render group(null, proxy)}
      {:else if section?.id === "queues"}
        {@render group(t("set.speedLimit"), speed)}
        {@render group(t("queue.title"), queueList)}
      {:else if section?.id === "categories"}
        {@render group(null, categories)}
      {:else if section?.id === "browser"}
        {@render group(t("set.browserStatus"), browserStatus)}
        {@render group(t("set.extensionInstall"), extension)}
        {@render group(t("set.capture"), capture)}
      {:else if section?.id === "video"}
        {@render group(t("set.tools"), videoTools)}
        {@render group(t("set.customPaths"), videoPaths)}
        {@render group(t("set.postprocess"), postprocess)}
      {:else if section?.id === "schedule"}
        {@render group(t("queue.main"), schedule)}
      {:else if section?.id === "notifications"}
        {@render group(null, notifications)}
      {:else if section?.id === "security"}
        {@render group(t("set.antivirus"), scanner)}
      {:else if section?.id === "advanced"}
        {@render group(t("set.backup"), backup)}
        {@render group(t("set.downloadList"), listBackup)}
        {@render group(t("set.diagnostics"), diagnostics)}
        {@render group(t("set.dataFolder"), dataFolder)}
      {:else if section?.id === "shortcuts"}
        {@render group(null, shortcuts)}
      {:else if section?.id === "about"}
        {@render group(null, about)}
      {/if}
    </div>
  </main>
</div>

{#if queueEdit}<QueueDialog queue={queueEdit} onclose={closeQueue} />{/if}

{#snippet group(title: string | null, rows: import("svelte").Snippet)}
  <section class="group">
    {#if title}<h2>{title}</h2>{/if}
    <div class="card">{@render rows()}</div>
  </section>
{/snippet}

{#snippet language()}
  <div class="item">
    <label for="s-lang">{t("set.language")}</label>
    <select id="s-lang" bind:value={s.language}>
      <option value="system">{t("set.languageSystem")}</option>
      <option value="id">Bahasa Indonesia</option>
      <option value="en">English</option>
    </select>
  </div>
{/snippet}

{#snippet startup()}
  <label class="item">
    <span>{t("set.autostart")}<small class="hint">{t("set.autostartHint")}</small></span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.autostart} />
  </label>
  <label class="item">
    <span>{t("set.clipboard")}</span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.clipboardMonitor} />
  </label>
{/snippet}

{#snippet theme()}
  <div class="theme-choices" role="radiogroup" aria-label={t("set.theme")}>
    {#each [["system", "set.themeSystem"], ["light", "set.themeLight"], ["dark", "set.themeDark"]] as const as [value, key] (value)}
      <label class="theme-choice" class:chosen={s.theme === value}>
        <input type="radio" name="theme" value={value} bind:group={s.theme} class="sr-only" />
        <span class="theme-preview" data-preview={value} aria-hidden="true">
          <span class="preview-nav"><i></i><i></i></span>
          <span class="preview-sheet"><i></i><i></i><i></i></span>
        </span>
        <span>{t(key)}</span>
      </label>
    {/each}
  </div>
{/snippet}

{#snippet accents()}
  <div class="swatches" role="radiogroup" aria-label={t("set.accent")}>
    {#each ACCENTS as accent (accent)}
      <label class="accent-choice" class:chosen={s.accent === accent} style:--swatch={SWATCH[accent]}>
        <input type="radio" name="accent" value={accent} bind:group={s.accent} class="sr-only" />
        <span class="swatch" aria-hidden="true">{#if s.accent === accent}<Icon name="completed" size={15} />{/if}</span>
        <span>{t(`accent.${accent}` as Key)}</span>
      </label>
    {/each}
  </div>
{/snippet}

{#snippet display()}
  <div class="item">
    <label for="s-density">{t("set.density")}</label>
    <select id="s-density" bind:value={s.density}>
      <option value="comfortable">{t("set.densityComfortable")}</option>
      <option value="compact">{t("set.densityCompact")}</option>
    </select>
  </div>
  <label class="item">
    <span>{t("set.glass")}<small class="hint">{t("set.glassHint")}</small></span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.glass} />
  </label>
{/snippet}

{#snippet folder()}
  <div class="item wide">
    <label for="s-dir">{t("set.folder")}</label>
    <div class="field">
      <input id="s-dir" bind:value={s.downloadDir} />
      <button onclick={browse}><Icon name="folder" />{t("new.browse")}</button>
    </div>
  </div>
{/snippet}

{#snippet behavior()}
  <div class="item">
    <label for="s-dup">{t("set.duplicate")}</label>
    <select id="s-dup" bind:value={s.duplicatePolicy}>
      <option value="rename">{t("set.dupRename")}</option>
      <option value="overwrite">{t("set.dupOverwrite")}</option>
      <option value="ask">{t("set.dupAsk")}</option>
    </select>
  </div>
  <label class="item">
    <span>{t("set.progressWindow")}<small class="hint">{t("set.progressWindowHint")}</small></span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.progressWindow} />
  </label>
{/snippet}

{#snippet connection()}
  <div class="item">
    <label for="s-conn">{t("set.connections")}</label>
    <div class="field">
      <input id="s-conn" type="range" min="1" max="32" bind:value={s.connections} />
      <output for="s-conn" class="num val">{s.connections}</output>
    </div>
  </div>
  <div class="item">
    <label for="s-timeout">{t("set.connectTimeout")}<small class="hint">{t("set.connectTimeoutHint")}</small></label>
    <input id="s-timeout" class="num short" type="number" min="1" max="300" step="1"
      value={s.connection.connectTimeoutSecs}
      onchange={(e) => {
        const value = e.currentTarget.valueAsNumber;
        if (Number.isFinite(value)) s.connection.connectTimeoutSecs = Math.max(1, Math.min(300, Math.round(value)));
        e.currentTarget.value = String(s.connection.connectTimeoutSecs);
      }}
    />
  </div>
  <div class="item">
    <label for="s-read-timeout">{t("set.readTimeout")}<small class="hint">{t("set.readTimeoutHint")}</small></label>
    <input id="s-read-timeout" class="num short" type="number" min="1" max="600"
      value={s.connection.readTimeoutSecs}
      onchange={(e) => {
        const value = e.currentTarget.valueAsNumber;
        if (Number.isFinite(value)) s.connection.readTimeoutSecs = Math.max(1, Math.min(600, Math.round(value)));
        e.currentTarget.value = String(s.connection.readTimeoutSecs);
      }}
    />
  </div>
  <div class="item">
    <label for="s-http">{t("set.httpVersion")}<small class="hint">{t("set.http3Hint")}</small></label>
    <select id="s-http" bind:value={s.connection.httpVersion}>
      <option value="auto">{t("set.httpAuto")}</option>
      <option value="http1">HTTP/1.1</option>
      <option value="http3" disabled={!app.info?.http3 || !s.connection.direct || Boolean(s.proxy?.trim())}>HTTP/3</option>
    </select>
  </div>
  <div class="item wide">
    <label for="s-user-agent">{t("set.userAgent")}</label>
    <input id="s-user-agent" class="mono" bind:value={s.connection.userAgent} maxlength="512" />
  </div>
{/snippet}

{#snippet proxy()}
  <div class="item wide">
    <label for="s-proxy">{t("set.proxyAddress")}<small class="hint">{t("set.proxyHint")}</small></label>
    <input id="s-proxy" class="mono" bind:value={s.proxy} placeholder="socks5://127.0.0.1:1080" />
  </div>
  <label class="item">
    <span>{t("set.directConnection")}<small class="hint">{t("set.directConnectionHint")}</small></span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.connection.direct} disabled={Boolean(s.proxy?.trim())} />
  </label>
  <div class="item wide">
    <label for="s-proxy-bypass">{t("set.proxyBypass")}<small class="hint">{t("set.proxyBypassHint")}</small></label>
    <input id="s-proxy-bypass" class="mono" value={s.connection.proxyBypass.join(", ")}
      onchange={(e) => (s.connection.proxyBypass = readList(e.currentTarget.value))} />
  </div>
{/snippet}

{#snippet speed()}
  <div class="item">
    <label for="s-limit">{t("set.speedLimit")}<small class="hint">{t("detail.limitHint")}</small></label>
    <input id="s-limit" class="num rate" bind:value={limit} onchange={applyLimit} placeholder={t("limit.off")} />
  </div>
{/snippet}

{#snippet queueList()}
  {#each queues as queue (queue.id)}
    <div class="item">
      <div>
        <span>{queue.name || t("queue.main")}</span>
        <small class="hint">{t("set.maxActive")} · <span class="num">{queue.maxActive}</span>{#if queue.schedule.enabled} · {t("set.scheduleEnabled")}{/if}</small>
      </div>
      <button onclick={() => editQueue(queue)}><Icon name="edit" size={14} />{t("queue.edit")}</button>
    </div>
  {/each}
  <div class="item actions">
    <button onclick={() => editQueue({ id: "", name: "", maxActive: 2, speedLimit: null, schedule: { enabled: false, start: null, stop: null, days: [] } })}>
      <Icon name="add" />{t("queue.new")}
    </button>
  </div>
{/snippet}

{#snippet categories()}
  <label class="item">
    <span>{t("set.categoryFolders")}</span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.categoryFolders} />
  </label>
  {#if s.categoryFolders && app.info}
    {#each CATEGORIES as category (category)}
      <div class="item">
        <div class="named">
          <Icon name={category} />
          <span>
            {t(`cat.${category}` as Key)}
            <small class="hint category-dir" title={s.categoryPaths[category] ?? s.downloadDir}>
              {s.categoryPaths[category] ?? `${app.info.categoryDirectories[category]}/`}
            </small>
          </span>
        </div>
        <div class="field">
          {#if s.categoryPaths[category]}
            <button onclick={() => delete s.categoryPaths[category]}>{t("set.resetFolder")}</button>
          {/if}
          <button onclick={() => browseCategory(category)}>{t("new.browse")}</button>
        </div>
      </div>
    {/each}
  {/if}
{/snippet}

{#snippet browserStatus()}
  <div class="item">
    <div class="named">
      <span class="indicator" class:ready={browser?.registered}></span>
      <span>
        <strong>{t(browser === null ? "loading" : browser.registered ? "set.bridgeRegistered" : "set.bridgeMissing")}</strong>
        <small class="hint">
          {browser?.lastSeen ? `${t("set.lastBrowserContact")} · ${new Date(browser.lastSeen * 1000).toLocaleString()}` : t("set.noBrowserContact")}
        </small>
      </span>
    </div>
    <button onclick={repairBrowser} disabled={browserBusy}><Icon name="refresh" size={14} />{t(browserBusy ? "set.repairingBridge" : "set.repairBridge")}</button>
  </div>
{/snippet}

{#snippet extension()}
  <div class="instructions">
    <ol>
      <li>{t("set.extensionStep1")}</li>
      <li>{t("set.extensionStep2")} <code>chrome://extensions</code> / <code>edge://extensions</code></li>
      <li>{t("set.extensionStep3")}</li>
      <li>{t("set.firefoxInstall")} <code>about:debugging#/runtime/this-firefox</code></li>
    </ol>
    <button onclick={() => action(api.openReleases)}><Icon name="down" size={14} />{t("set.releases")}</button>
  </div>
{/snippet}

{#snippet capture()}
  <label class="item">
    <span>{t("set.confirmNew")}</span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.confirmNew} />
  </label>
  <label class="item">
    <span>{t("set.manageCapture")}<small class="hint">{t("set.manageCaptureHint")}</small></span>
    <input type="checkbox" class="switch" role="switch" checked={s.capture !== null}
      onchange={(e) => (s.capture = e.currentTarget.checked ? basicCapture() : null)} />
  </label>
  {#if s.capture}
    <label class="item">
      <span>{t("set.captureEnabled")}</span>
      <input type="checkbox" class="switch" role="switch" bind:checked={s.capture.enabled} />
    </label>
    <div class="item">
      <label for="s-min-size">{t("set.captureMinSize")}</label>
      <input id="s-min-size" class="num short" type="number" min="0" max="97656250"
        value={Math.round(s.capture.minSize / 1024)}
        onchange={(e) => { if (s.capture && Number.isFinite(e.currentTarget.valueAsNumber)) s.capture.minSize = Math.min(100_000_000_000, Math.max(0, Math.round(e.currentTarget.valueAsNumber) * 1024)); }} />
    </div>
    <div class="item wide">
      <label for="s-capture-ext">{t("set.captureTypes")}<small class="hint">{t("set.anyType")}</small></label>
      <input id="s-capture-ext" value={s.capture.extensions.join(", ")}
        onchange={(e) => { if (s.capture) s.capture.extensions = readList(e.currentTarget.value).map((ext) => ext.replace(/^\./, "")); }} placeholder="zip, pdf, mp4" />
    </div>
    <div class="item wide">
      <label for="s-capture-mime">{t("set.captureMimes")}<small class="hint">{t("set.anyType")}</small></label>
      <input id="s-capture-mime" value={s.capture.mimeTypes.join(", ")}
        onchange={(e) => { if (s.capture) s.capture.mimeTypes = readList(e.currentTarget.value); }} placeholder="video/*, application/pdf" />
    </div>
    <div class="item wide">
      <label for="s-capture-domains">{t("set.captureDomains")}</label>
      <input id="s-capture-domains" value={s.capture.excludedDomains.join(", ")}
        onchange={(e) => { if (s.capture) s.capture.excludedDomains = readList(e.currentTarget.value); }} placeholder="example.com" />
    </div>
    <label class="item">
      <span>{t("set.captureVideo")}</span>
      <input type="checkbox" class="switch" role="switch" bind:checked={s.capture.videoButton} />
    </label>
    <div class="item">
      <label for="s-video-duration">{t("set.captureVideoDuration")}</label>
      <input id="s-video-duration" class="num short" type="number" min="0" max="36000"
        value={s.capture.minVideoDuration}
        onchange={(e) => { if (s.capture && Number.isFinite(e.currentTarget.valueAsNumber)) s.capture.minVideoDuration = Math.min(36_000, Math.max(0, Math.round(e.currentTarget.valueAsNumber))); }} />
    </div>
  {/if}
{/snippet}

{#snippet videoTools()}
  {#each tools as tool (tool.tool)}
    <div class="item">
      <div class="tool">
        <span class="mono">{tool.tool}</span>
        {#if tool.path}<small class="hint tool-path" title={tool.path}>{tool.path}</small>{/if}
      </div>
      <span class="tool-status" class:available={Boolean(tool.path)}>{toolText(tool)}</span>
    </div>
  {/each}
  <label class="item">
    <span>{t("set.autoUpdate")}</span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.video.autoUpdate} />
  </label>
  <div class="item actions">
    <button onclick={installTools} disabled={updating !== null}><Icon name="down" size={14} />{t(updating === "all" ? "set.installingTools" : "set.installTools")}</button>
    {#if ytdlp && (installing || ytdlp.managed)}
      <button onclick={updateYtdlp} disabled={updating !== null}>
        {t(updating === "yt-dlp" ? "set.fetchingYtdlp" : installing ? "set.installYtdlp" : "set.updateYtdlp")}
      </button>
    {/if}
    {#if toolNote}<small class="hint tool-note" aria-live="polite">{toolNote}</small>{/if}
  </div>
{/snippet}

{#snippet videoPaths()}
  <div class="item wide">
    <label for="s-ytdlp">{t("set.ytdlpPath")}<small class="hint">{t("set.pathHint")}</small></label>
    <input id="s-ytdlp" class="mono" bind:value={s.video.ytdlpPath} />
  </div>
  <div class="item wide">
    <label for="s-ffmpeg">{t("set.ffmpegPath")}<small class="hint">{t("set.pathHint")}</small></label>
    <input id="s-ffmpeg" class="mono" bind:value={s.video.ffmpegPath} />
  </div>
{/snippet}

{#snippet postprocess()}
  {#each [
    ["embedSubtitles", "set.subtitles"], ["embedMetadata", "set.metadata"],
    ["embedThumbnail", "set.thumbnail"], ["embedChapters", "set.chapters"],
  ] as const as [key, label] (key)}
    <label class="item">
      <span>{t(label)}</span>
      <input type="checkbox" class="switch" role="switch" bind:checked={s.video.postprocess[key]} />
    </label>
  {/each}
  {#if s.video.postprocess.embedSubtitles}
    <div class="item wide">
      <label for="s-sub-langs">{t("set.subtitleLanguages")}</label>
      <input id="s-sub-langs" bind:value={s.video.postprocess.subtitleLanguages} placeholder="id,en.*" maxlength="256" />
    </div>
  {/if}
  <div class="item">
    <label for="s-sponsorblock">SponsorBlock<small class="hint">{t("set.sponsorblockHint")}</small></label>
    <select id="s-sponsorblock" bind:value={s.video.postprocess.sponsorblock}>
      <option value="off">{t("set.afterNothing")}</option>
      <option value="mark">{t("set.sponsorMark")}</option>
      <option value="remove">{t("set.sponsorRemove")}</option>
    </select>
  </div>
{/snippet}

{#snippet schedule()}
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
    <span id="s-days">{t("set.scheduleDays")}<small class="hint">{t("set.scheduleDaysHint")}</small></span>
    <div class="days" role="group" aria-labelledby="s-days">
      {#each DAYS as day (day)}
        <button aria-pressed={s.schedule.days.includes(day)} disabled={!s.schedule.enabled} onclick={() => toggleDay(day)}>
          {t(`day.${day}` as Key)}
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
{/snippet}

{#snippet notifications()}
  <label class="item">
    <span>{t("set.notifications")}</span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.notifications} />
  </label>
  <label class="item">
    <span>{t("set.sound")}</span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.sound} />
  </label>
{/snippet}

{#snippet backup()}
  <div class="item wide">
    <span>{t("set.backupHint")}</span>
    <div class="field">
      <button onclick={() => transferSettings("export")} disabled={fileBusy}><Icon name="up" size={14} />{t("set.export")}</button>
      <button onclick={() => transferSettings("import")} disabled={fileBusy}><Icon name="down" size={14} />{t("set.import")}</button>
    </div>
  </div>
{/snippet}

{#snippet scanner()}
  <label class="item">
    <span>{t("set.scanDownloads")}<small class="hint">{t("set.scanHint")}</small></span>
    <input type="checkbox" class="switch" role="switch" bind:checked={s.scanner.enabled} />
  </label>
  {#if s.scanner.enabled}
    <div class="item wide">
      <label for="s-scan-ext">{t("set.scanExtensions")}<small class="hint">{t("set.anyType")}</small></label>
      <input id="s-scan-ext" value={s.scanner.extensions.join(", ")}
        onchange={(e) => (s.scanner.extensions = readList(e.currentTarget.value))} placeholder="exe, msi, zip" />
    </div>
  {/if}
{/snippet}

{#snippet listBackup()}
  <div class="item wide">
    <span>{t("set.listBackupHint")}</span>
    <div class="field">
      <button onclick={() => transferList("export")} disabled={fileBusy}>{t("set.export")}</button>
      <button onclick={() => transferList("import")} disabled={fileBusy}>{t("set.import")}</button>
    </div>
  </div>
{/snippet}

{#snippet diagnostics()}
  <div class="item">
    <span>{t("set.diagnosticsHint")}</span>
    <button onclick={() => transferList("diagnostics")} disabled={fileBusy}>{t("set.exportZip")}</button>
  </div>
{/snippet}

{#snippet dataFolder()}
  <div class="item wide">
    <code>{app.info?.dataDir ?? ""}</code>
    <div class="field"><button onclick={() => action(api.openDataDirectory)}><Icon name="folder" />{t("act.folder")}</button></div>
  </div>
{/snippet}

{#snippet shortcuts()}
  {#each SHORTCUTS as [label, combo] (label)}
    <div class="item">
      <span>{t(label)}</span>
      <div class="keys">
        {#each combo.split(" / ") as alias (alias)}
          <span>{#each alias.split("+") as key (key)}<kbd>{key}</kbd>{/each}</span>
        {/each}
      </div>
    </div>
  {/each}
{/snippet}

{#snippet about()}
  <div class="about">
    <img src={mark} alt="" width="72" height="72" />
    <strong>Oxyd Download Manager</strong>
    <span class="muted">{t("set.version")} <span class="num">{app.version}</span> · MIT</span>
    <div class="field">
      <button class="primary" onclick={() => action(api.openReleases)}>{t("set.releases")}</button>
      <button onclick={() => action(api.openProject)}>{t("set.sourceCode")}</button>
    </div>
  </div>
{/snippet}

<style>
  .settings-shell {
    display: grid;
    grid-template-columns: clamp(164px, 27%, 224px) minmax(0, 1fr);
    gap: 8px;
    height: 100%;
    padding: 8px;
    background: radial-gradient(70% 85% at 0% 0%, var(--glass-light), transparent), var(--ground);
  }
  .navigation, main {
    min-height: 0;
    min-width: 0;
    border-radius: var(--radius-lg);
    box-shadow: var(--sheet-shadow);
  }
  .navigation {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 10px 10px;
    background: linear-gradient(155deg, var(--glass-edge), transparent 50%), var(--panel);
    backdrop-filter: blur(18px) saturate(145%);
  }
  main {
    overflow: auto;
    background: var(--surface);
    overscroll-behavior: contain;
  }
  :global(.no-glass) .settings-shell { background: var(--ground); }
  :global(.no-glass) .navigation { background: var(--surface); backdrop-filter: none; }
  @media (prefers-reduced-transparency: reduce) {
    .settings-shell { background: var(--ground); }
    .navigation { background: var(--surface); backdrop-filter: none; }
  }
  .identity, .search, nav button, .named {
    display: flex;
    gap: 9px;
    align-items: center;
  }
  .identity { padding: 0 5px; font-size: 15px; font-weight: 700; }
  .search {
    min-width: 0;
    padding: 0 8px;
    border: 1px solid var(--control);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
  }
  .search:focus-within { border-color: var(--accent); }
  .search input {
    width: 100%;
    min-width: 0;
    padding-inline: 0;
    border: 0;
    outline: none;
    background: transparent;
  }
  nav {
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 2px;
  }
  nav button {
    flex: none;
    width: 100%;
    height: auto;
    min-height: 34px;
    justify-content: flex-start;
    padding: 4px 8px;
    border-color: transparent;
    text-align: start;
    white-space: normal;
    line-height: 1.3;
    color: var(--muted);
  }
  .navigation :global(svg), .named :global(svg) { flex: none; }
  nav button.separate { margin-block-start: 9px; }
  nav button[aria-current="page"] {
    border-color: var(--accent-line);
    color: var(--text);
    background: var(--accent-soft);
  }
  nav button[aria-current="page"] :global(svg) { color: var(--accent); }

  .column { max-width: 860px; margin: 0 auto; padding: 26px clamp(12px, 3.8vw, 38px) 32px; }
  header { margin-bottom: 22px; }
  h1 { margin: 0; font-size: 24px; line-height: 30px; letter-spacing: -0.025em; }
  header p { margin: 4px 0 0; font-size: 12px; }
  .feedback {
    margin: 0 0 15px;
    padding: 10px 12px;
    background: var(--neg-bg);
    color: var(--negative);
    border-radius: var(--radius);
    overflow-wrap: anywhere;
  }
  .group + .group { margin-top: 19px; }
  .group h2 { margin: 0 0 7px 2px; font-size: 12px; font-weight: 650; color: var(--muted); }
  .card {
    padding: 0 14px;
    border-radius: var(--radius-lg);
    background: var(--raised);
    box-shadow: var(--sheet-shadow);
  }
  .item {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 14px;
    min-height: 52px;
    padding: 10px 0;
  }
  .item + .item { border-top: 1px solid var(--rule); }
  label.item { cursor: pointer; }
  .item > :first-child { flex: 1 1 145px; min-width: 0; }
  .item > :is(input, select) { max-width: 100%; }
  .wide { flex-direction: column; align-items: stretch; }
  .wide > :first-child { flex: none; }
  .wide code, .instructions code { overflow-wrap: anywhere; }
  .hint { display: block; color: var(--muted); font-size: 12px; line-height: 17px; }
  .field, .actions, .keys, .days { display: flex; align-items: center; flex-wrap: wrap; gap: 7px; }
  .wide .field input { flex: 1 1 120px; min-width: 0; }
  .item.actions { justify-content: flex-start; }
  .item.actions > :first-child { flex: none; }
  .short { width: 80px; text-align: right; }
  .rate { width: 120px; }
  .val { min-width: 20px; text-align: right; }
  .days button { min-width: 33px; padding: 0 5px; }
  .days button[aria-pressed="true"] { color: var(--accent); border-color: var(--accent-line); background: var(--accent-soft); }
  .keys span { display: inline-flex; gap: 3px; }

  .theme-choices { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; padding: 15px 0; }
  .theme-choice, .accent-choice { display: grid; gap: 6px; cursor: pointer; color: var(--muted); font-size: 12px; }
  .theme-choice.chosen, .accent-choice.chosen { font-weight: 650; color: var(--text); }
  .theme-preview {
    display: grid;
    grid-template-columns: 1fr 2.2fr;
    gap: 4px;
    padding: 6px;
    min-height: 57px;
    height: clamp(57px, 10vw, 90px);
    border: 1px solid var(--control);
    border-radius: var(--radius);
    background: #dae4e1;
  }
  .theme-preview[data-preview="dark"] { background: #172623; }
  .theme-preview[data-preview="system"] { background: linear-gradient(115deg, #dae4e1 46%, #172623 46%); }
  .theme-choice.chosen .theme-preview,
  .theme-choice:has(input:focus-visible) .theme-preview { outline: 2px solid var(--accent); outline-offset: 2px; }
  .preview-nav, .preview-sheet { padding: 6px 4px; min-width: 0; border-radius: 4px; background: #f8fbfa; }
  .theme-preview[data-preview="dark"] :is(.preview-nav, .preview-sheet),
  .theme-preview[data-preview="system"] .preview-sheet { background: #263a35; }
  .theme-preview i { display: block; height: 3px; border-radius: 2px; background: #8ba79f; opacity: .65; margin-bottom: 5px; }
  .preview-sheet i:first-child { width: 60%; background: var(--accent-solid); opacity: 1; }
  .swatches { display: flex; flex-wrap: wrap; align-items: flex-start; gap: 13px 15px; padding: 15px 0; }
  .accent-choice { justify-items: center; min-width: 42px; text-align: center; }
  .swatch { width: 27px; height: 27px; display: grid; place-items: center; border-radius: 50%; color: #fff; background: var(--swatch); }
  .accent-choice.chosen .swatch,
  .accent-choice:has(input:focus-visible) .swatch { outline: 2px solid var(--swatch); outline-offset: 2px; }

  .indicator { width: 8px; height: 8px; flex: none; border-radius: 50%; background: var(--warning); }
  .indicator.ready { background: var(--positive); }
  .instructions { padding: 12px 0; }
  .instructions ol { margin: 0 0 12px; padding-inline-start: 20px; }
  .instructions li + li { margin-top: 5px; }
  .tool-path, .category-dir { overflow-wrap: anywhere; }
  .tool-status { background: var(--warn-bg); color: var(--warning); border-radius: 7px; padding: 2px 7px; font-size: 12px; }
  .tool-status.available { background: var(--pos-bg); color: var(--positive); }
  .tool-note { flex-basis: 100%; }
  .about { display: grid; justify-items: center; text-align: center; gap: 9px; padding: 32px 0; }
  .about strong { font-size: 20px; }
  .about .field { justify-content: center; margin-top: 5px; }
</style>
