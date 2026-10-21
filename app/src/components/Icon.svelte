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
