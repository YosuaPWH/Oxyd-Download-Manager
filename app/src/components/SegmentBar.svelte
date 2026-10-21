<script lang="ts">
  import type { Segment } from "../lib/types";

  let {
    segments,
    total,
    downloaded = 0,
  }: { segments: Segment[]; total: number | null; downloaded?: number } = $props();

  // Unknown-length streams use u64::MAX as their end.
  const size = $derived(
    total ?? segments.reduce((m, s) => Math.max(m, s.start + s.downloaded), 0),
  );
</script>

<div class="bar" role="img" aria-label="segments">
  {#if !segments.length && total && downloaded >= total}
    <div class="seg" style:left="0" style:width="100%"><div class="fill" style:width="100%"></div></div>
  {:else if size > 0}
    {#each segments as s, i (i)}
      {@const end = Math.min(s.end, size - 1)}
      {@const len = Math.max(end - s.start + 1, 0)}
      <div
        class="seg"
        style:left="{(s.start / size) * 100}%"
        style:width="{(len / size) * 100}%"
        title="#{i + 1}: {s.start}–{end}"
      >
        <div class="fill" style:width="{len ? Math.min(100, (s.downloaded / len) * 100) : 100}%"></div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .bar {
    position: relative;
    height: 18px;
    border-radius: 4px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .seg {
    position: absolute;
    top: 0;
    bottom: 0;
    border-right: 1px solid var(--panel);
  }
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
    transition: width 0.25s linear;
  }
</style>
