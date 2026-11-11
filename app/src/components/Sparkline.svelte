<script lang="ts">
  /** Samples, oldest first; drawn against their own maximum. */
  let { samples, height = 34 }: { samples: number[]; height?: number } = $props();

  const W = 180;
  const line = $derived.by(() => {
    if (samples.length < 2) return "";
    const max = Math.max(1, ...samples);
    const n = 60;
    return samples
      .map((v, i) => {
        const x = (W * (n - (samples.length - 1 - i))) / n;
        const y = height - 2 - (v / max) * (height - 5);
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  });
  const first = $derived(line.split(" ")[0]?.split(",")[0] ?? "0");
</script>

<svg viewBox="0 0 {W} {height}" width="100%" {height} preserveAspectRatio="none" aria-hidden="true">
  {#if line}
    <polygon class="area" points="{first},{height} {line} {W},{height}" />
    <polyline points={line} />
  {:else}
    <line x1="0" x2={W} y1={height - 2} y2={height - 2} />
  {/if}
</svg>

<style>
  svg {
    display: block;
  }
  .area {
    fill: var(--accent-soft);
    stroke: none;
  }
  polyline {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  line {
    stroke: var(--track);
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
  }
</style>
