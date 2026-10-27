<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onclose,
    width = 520,
    children,
    footer,
  }: {
    title: string;
    onclose: () => void;
    width?: number;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  let dialog: HTMLDivElement;

  $effect(() => {
    const prev = document.activeElement as HTMLElement | null;
    // Start in the first field of the dialog body so typing works right away. querySelector
    // returns the first match in document order, which would be the close button in the
    // header. Dialogs without fields keep that safe default.
    const first =
      dialog.querySelector<HTMLElement>(
        ".content input:not([readonly]):not([type='hidden']), .content select, .content textarea",
      ) ?? dialog.querySelector<HTMLElement>("button");
    first?.focus();
    return () => prev?.focus();
  });

  function windowKeydown(e: KeyboardEvent) {
    // Only the top-most dialog reacts to Escape.
    const all = document.querySelectorAll(".modal");
    if (e.key === "Escape" && all[all.length - 1] === dialog) {
      e.stopPropagation();
      onclose();
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Tab") {
      // Keep focus inside the dialog.
      const items = [
        ...dialog.querySelectorAll<HTMLElement>(
          "input, select, button, textarea, [tabindex]:not([tabindex='-1'])",
        ),
      ].filter((el) => !(el as HTMLButtonElement).disabled);
      if (!items.length) return;
      const first = items[0];
      const last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        last.focus();
        e.preventDefault();
      } else if (!e.shiftKey && document.activeElement === last) {
        first.focus();
        e.preventDefault();
      }
    }
  }
</script>

<svelte:window onkeydown={windowKeydown} />

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    style:width="{width}px"
    bind:this={dialog}
    onkeydown={keydown}
  >
    <header>
      <h2>{title}</h2>
      <button class="icon" aria-label="Close" onclick={onclose}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </header>
    <div class="content">{@render children()}</div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    display: grid;
    place-items: center;
    z-index: 50;
  }
  .modal {
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 32px);
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--float-shadow);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 14px 0 20px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .content {
    padding: 16px 20px 20px;
    overflow: auto;
  }
  footer {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    padding: 14px 20px;
    border-top: 1px solid var(--rule);
  }
</style>
