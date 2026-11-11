<script lang="ts">
  import type { Category } from "../lib/types";
  import Icon from "./Icon.svelte";

  /** The square in front of a file name: its type as an icon, tinted per category. */
  let { category }: { category: Category } = $props();
</script>

<span class="tile {category}"><Icon name={category} size={15} /></span>

<style>
  .tile {
    flex: none;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius);
    color: var(--tint, var(--muted));
    background: color-mix(in srgb, var(--tint, var(--muted)) 12%, transparent);
  }
  /* The type tints reuse the lane ramp: no extra colours to keep in tune with the themes. */
  .video {
    --tint: var(--lane-8);
  }
  .music {
    --tint: var(--lane-4);
  }
  .image {
    --tint: var(--lane-6);
  }
  .document {
    --tint: var(--lane-7);
  }
  .archive {
    --tint: var(--lane-2);
  }
  .program {
    --tint: var(--lane-1);
  }
  :global(:root[data-density="compact"]) .tile {
    width: 22px;
    height: 22px;
    border-radius: 6px;
  }
</style>
