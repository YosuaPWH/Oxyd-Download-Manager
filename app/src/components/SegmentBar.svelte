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

<!-- One cell per connection's range; a finished range turns grey, a running one fills in the accent. -->
<div class="bar" role="img" aria-label="{segments.length} segments">
  {#if !segments.length && total && downloaded >= total}
    <div class="seg" style:left="0" style:width="100%"><div class="fill done" style:width="100%"></div></div>
  {:else if size > 0}
    {#each segments as s, i (i)}
      {@const end = Math.min(s.end, size - 1)}
      {@const len = Math.max(end - s.start + 1, 0)}
      {@const p = len ? Math.min(100, (s.downloaded / len) * 100) : 100}
      <div
        class="seg"
        style:left="{(s.start / size) * 100}%"
        style:width="{(len / size) * 100}%"
        title="#{i + 1}: {s.start}–{end} · {p.toFixed(0)}%"
      >
        <div class="fill" class:done={p >= 100} style:width="{p}%"></div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .bar {
    position: relative;
    height: 12px;
    background: var(--track);
    overflow: hidden;
  }
  .seg {
    position: absolute;
    top: 0;
    bottom: 0;
    box-shadow: inset -2px 0 0 var(--surface);
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.25s linear;
  }
  .fill.done {
    background: var(--done);
  }
</style>
