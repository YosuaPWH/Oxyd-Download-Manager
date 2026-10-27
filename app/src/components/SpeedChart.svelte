<script lang="ts">
  /** Speed samples, one per second, oldest first (up to 61: the last minute). */
  let { samples, title }: { samples: number[]; title: string } = $props();

  const W = 336;
  const H = 152;
  const left = 36;
  const top = 8;
  const plot = 120;

  // Round steps for the scale: 4 ticks with the top one above the highest sample.
  const scale = $derived.by(() => {
    const max = Math.max(1, ...samples);
    const mb = max >= 1024 * 1024;
    const unit = mb ? 1024 * 1024 : 1024;
    const raw = max / unit / 3;
    const mag = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s * 3 >= max / unit) ?? 10 * mag;
    return { unit, step, top: step * 3, label: mb ? "MB/s" : "KB/s" };
  });

  const points = $derived(
    samples
      .map((v, i) => {
        const x = left + ((W - left) * (60 - (samples.length - 1 - i))) / 60;
        const y = top + plot - (v / scale.unit / scale.top) * plot;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" "),
  );

  const ticks = $derived([0, 1, 2, 3].map((i) => ({ y: top + plot - (i * plot) / 3, v: +(i * scale.step).toFixed(2) })));
  const now = $derived(samples.length ? samples[samples.length - 1] : 0);
</script>

<div class="caption"><span>{title}</span><span>{scale.label}</span></div>
<svg
  viewBox="0 0 {W} {H}"
  width="100%"
  role="img"
  aria-label="Speed over the last minute, now {(now / scale.unit).toFixed(1)} {scale.label}"
>
  {#each ticks as tk, i (i)}
    <line x1={left} x2={W} y1={tk.y} y2={tk.y} class={i ? "grid" : "base"} />
    <text x={left - 8} y={tk.y + 4} text-anchor="end">{tk.v}</text>
  {/each}
  {#if samples.length > 1}
    <polyline {points} />
  {/if}
  <text x={left} y={H - 4}>−60s</text>
  <text x={left + (W - left) / 2} y={H - 4} text-anchor="middle">−30s</text>
  <text x={W} y={H - 4} text-anchor="end">now</text>
</svg>

<style>
  svg {
    display: block;
    max-width: 420px;
    overflow: visible;
  }
  .grid {
    stroke: var(--rule);
  }
  .base {
    stroke: var(--control);
  }
  text {
    fill: var(--muted);
    font-family: var(--font-num);
    font-size: 11px;
  }
  .caption {
    display: flex;
    justify-content: space-between;
    margin-bottom: 8px;
    font-size: 12px;
    color: var(--muted);
  }
  polyline {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linejoin: round;
  }
</style>
