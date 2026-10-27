<script lang="ts">
  import { t } from "../lib/i18n.svelte";
  import { app } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";

  // The mark is a ring cut into eight segments, one of them lit: a file fetched in parallel.
  const arcs = [
    "M14 3A11 11 0 0 1 21.78 6.22",
    "M22.5 7A11 11 0 0 1 25 14",
    "M25 15A11 11 0 0 1 21.78 21.78",
    "M21 22.5A11 11 0 0 1 14 25",
    "M13 25A11 11 0 0 1 6.22 21.78",
    "M5.5 21A11 11 0 0 1 3 14",
    "M3 13A11 11 0 0 1 6.22 6.22",
    "M7 5.5A11 11 0 0 1 13 3",
  ];
</script>

<nav aria-label="Oxyd">
  <svg class="mark" width="28" height="28" viewBox="0 0 28 28" role="img" aria-label="Oxyd">
    {#each arcs as d, i (i)}
      <path {d} class:lit={i === 0} />
    {/each}
  </svg>
  <button
    class="item"
    aria-label={t("page.downloads")}
    title={t("page.downloads")}
    aria-current={app.page === "downloads" ? "page" : undefined}
    onclick={() => (app.page = "downloads")}
  >
    <Icon name="active" size={18} />
  </button>
  <span class="grow"></span>
  <button
    class="item"
    aria-label={t("tb.settings")}
    title="{t('tb.settings')} (Ctrl+,)"
    aria-current={app.page === "settings" ? "page" : undefined}
    onclick={() => (app.page = "settings")}
  >
    <Icon name="settings" size={18} />
  </button>
</nav>

<style>
  nav {
    width: 56px;
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 14px 0;
    border-right: 1px solid var(--border);
  }
  .mark {
    margin-bottom: 18px;
    fill: none;
    stroke: var(--text);
    stroke-width: 3;
  }
  .mark .lit {
    stroke: var(--accent);
  }
  .grow {
    flex: 1;
  }
  .item {
    width: 40px;
    height: 40px;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    color: var(--muted);
  }
  .item:hover:not(:disabled) {
    color: var(--text);
  }
  /* The move: selection is a 1px accent line, here on the rail's own edge. */
  .item {
    position: relative;
  }
  .item[aria-current="page"] {
    color: var(--text);
  }
  .item[aria-current="page"]::after {
    content: "";
    position: absolute;
    top: 8px;
    bottom: 8px;
    right: -8px;
    width: 1px;
    background: var(--accent);
  }
</style>
