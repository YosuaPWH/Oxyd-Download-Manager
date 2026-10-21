<script lang="ts">
  import { app, visible, type View } from "../lib/store.svelte";
  import { t, type Key } from "../lib/i18n.svelte";
  import Icon from "./Icon.svelte";

  const status: { view: View; key: Key }[] = [
    { view: "all", key: "nav.all" },
    { view: "active", key: "nav.active" },
    { view: "unfinished", key: "nav.queued" },
    { view: "completed", key: "nav.completed" },
  ];
  const cats: { view: View; key: Key }[] = [
    { view: "video", key: "cat.video" },
    { view: "music", key: "cat.music" },
    { view: "document", key: "cat.document" },
    { view: "archive", key: "cat.archive" },
    { view: "program", key: "cat.program" },
    { view: "other", key: "cat.other" },
  ];

  function count(v: View) {
    return app.downloads.filter((d) => visible(d, v)).length;
  }
</script>

{#snippet item(v: View, key: Key)}
  <button class="item" class:on={app.view === v} onclick={() => (app.view = v)} aria-pressed={app.view === v}>
    <Icon name={v} />
    <span class="label">{t(key)}</span>
    <span class="count">{count(v) || ""}</span>
  </button>
{/snippet}

<nav aria-label={t("nav.status")}>
  <h3>{t("nav.status")}</h3>
  {#each status as s (s.view)}{@render item(s.view, s.key)}{/each}
  <h3>{t("nav.categories")}</h3>
  {#each cats as c (c.view)}{@render item(c.view, c.key)}{/each}
</nav>

<style>
  nav {
    width: 196px;
    flex-shrink: 0;
    padding: 10px 8px;
    border-right: 1px solid var(--border);
    background: var(--panel);
    overflow-y: auto;
  }
  h3 {
    margin: 12px 10px 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  h3:first-child {
    margin-top: 2px;
  }
  .item {
    width: 100%;
    border: none;
    background: transparent;
    padding: 7px 10px;
    border-radius: 6px;
    justify-content: flex-start;
    gap: 10px;
    color: var(--text);
  }
  .item.on {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .label {
    flex: 1;
    text-align: left;
  }
  .count {
    font-size: 11px;
    color: var(--muted);
  }
</style>
