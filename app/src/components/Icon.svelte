<script lang="ts">
  const paths: Record<string, string> = {
    add: "M12 5v14M5 12h14",
    play: "M7 5l12 7-12 7z",
    pause: "M8 5v14M16 5v14",
    trash: "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13",
    settings:
      "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
    all: "M4 6h16M4 12h16M4 18h16",
    active: "M12 3v12M7 10l5 5 5-5M5 21h14",
    unfinished: "M12 8v4l3 2M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z",
    completed: "M5 13l4 4L19 7",
    video: "M4 6h11v12H4zM15 10l5-3v10l-5-3",
    music: "M9 18V5l11-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM20 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z",
    document: "M7 3h7l5 5v13H7zM14 3v5h5M10 13h6M10 17h6",
    archive: "M4 7h16v13H4zM3 3h18v4H3zM10 11h4",
    program: "M4 5h16v14H4zM4 9h16M8 13l2 2-2 2M12 17h4",
    other: "M6 3h9l4 4v14H6zM15 3v4h4",
    folder: "M3 6h6l2 2h10v11H3z",
    file: "M6 3h9l4 4v14H6z",
    copy: "M9 9h11v11H9zM5 15H4V4h11v1",
    refresh: "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7",
    up: "M12 19V5M5 12l7-7 7 7",
    down: "M12 5v14M5 12l7 7 7-7",
    x: "M6 6l12 12M18 6L6 18",
    stop: "M6 6h12v12H6z",
    link: "M10 14a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1M14 10a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1",
    clipboard: "M9 4h6v3H9zM7 5H5v16h14V5h-2",
    globe: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18",
    pip: "M3 5h18v14H3zM12 12h7v5h-7z",
    queue: "M4 6h12M4 12h12M4 18h7M19 15v6M16 18h6",
    wrench: "M14.5 6.5a4 4 0 0 0-5.3 5.3L3 18l3 3 6.2-6.2a4 4 0 0 0 5.3-5.3L15 12l-3-3z",
    magnet: "M6 3v9a6 6 0 0 0 12 0V3h-4v9a2 2 0 0 1-4 0V3zM6 7h4M14 7h4",
    edit: "M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4",
    image: "M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15.5 9.5h.01",
    layers: "M12 3l9 5-9 5-9-5zM3 13l9 5 9-5",
    gauge: "M4 18a8 8 0 1 1 16 0M12 18l4-6",
    search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-3.5-3.5",
    panel: "M4 5h16v14H4zM4 14h16",
    chevronDown: "M6 9l6 6 6-6",
    chevronUp: "M6 15l6-6 6 6",
    clock: "M12 7v5l3 2M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z",
    calendar: "M4 6h16v14H4zM4 10h16M8 3v4M16 3v4",
    more: "M5 12h.01M12 12h.01M19 12h.01",
    alert: "M12 8v5M12 16.5h.01M12 3l9.5 17h-19z",
    merge: "M7 4v5a5 5 0 0 0 5 5 5 5 0 0 1 5 5v1M17 4v5a5 5 0 0 1-1.5 3.5",
    sweep: "M4 6h16M4 12h10M4 18h7M16 15l5 5M21 15l-5 5",
    playAll: "M4 5l9 7-9 7zM14 5l7 7-7 7",
    pauseAll: "M5 5v14M10 5v14M15 5v14M20 5v14",
  };

  let { name, size = 16 }: { name: string; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name] ?? paths.other} />
</svg>
