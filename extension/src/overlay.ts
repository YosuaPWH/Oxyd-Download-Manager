export const OVERLAY_KEY = "videoOverlay";
const CORNERS = ["top-right", "top-left", "bottom-right", "bottom-left"] as const;
type Corner = (typeof CORNERS)[number];

export interface Layout {
  compact: boolean;
  corner: Corner;
}

export const DEFAULT_LAYOUT: Layout = { compact: false, corner: "top-right" };

export interface OverlaySettings {
  defaults: Layout;
  sites: Record<string, Partial<Layout>>;
}

export function siteHost(url: string | undefined): string | undefined {
  if (!url) return undefined;
  try {
    const parsed = new URL(url);
    if (parsed.protocol === "http:" || parsed.protocol === "https:") {
      return parsed.hostname.replace(/\.$/, "");
    }
  } catch {
    // A browser page without a usable website URL only gets the default layout.
  }
  return undefined;
}

export function layoutOverride(raw: unknown): Partial<Layout> {
  const input = raw as Partial<Layout> | undefined | null;
  const corner = CORNERS.find((value) => value === input?.corner);
  return {
    ...(typeof input?.compact === "boolean" ? { compact: input.compact } : {}),
    ...(corner ? { corner } : {}),
  };
}

export function overlaySettings(raw: unknown): OverlaySettings {
  const input = raw as Partial<OverlaySettings> | undefined | null;
  const sites = Object.fromEntries(
    Object.entries(input?.sites ?? {}).map(([host, rule]) => [host, layoutOverride(rule)]),
  );
  return { defaults: { ...DEFAULT_LAYOUT, ...layoutOverride(input?.defaults) }, sites };
}

export function siteLayout(prefs: OverlaySettings, host: string | undefined): Partial<Layout> {
  return host && Object.hasOwn(prefs.sites, host) ? prefs.sites[host]! : {};
}

export function effectiveLayout(prefs: OverlaySettings, host: string | undefined): Layout {
  return { ...prefs.defaults, ...siteLayout(prefs, host) };
}

/** Keep the controls and the scrollable menu inside the viewport at every corner. */
export function placeOverlay(corner: Corner, player: Pick<DOMRect, "left" | "top" | "right" | "bottom">,
                             viewport: { width: number; height: number }, bar: { width: number; height: number }) {
  const margin = 8;
  const inset = 10;
  const gap = 6;
  const left = corner.endsWith("left");
  const top = corner.startsWith("top");
  const barX = left ? Math.max(0, player.left) + inset : Math.min(player.right, viewport.width) - bar.width - inset;
  const barY = top ? Math.max(0, player.top) + inset : Math.min(player.bottom, viewport.height) - bar.height - inset;
  const x = Math.max(margin, Math.min(barX, viewport.width - bar.width - margin));
  const y = Math.max(margin, Math.min(barY, viewport.height - bar.height - margin));
  const width = Math.max(0, Math.min(300, viewport.width - 2 * margin));
  const menuX = left ? x : x + bar.width - width;
  const menuLeft = Math.max(margin, Math.min(menuX, viewport.width - width - margin)) - x;
  const above = y - gap - margin;
  const below = viewport.height - y - bar.height - gap - margin;
  const up = below < 360 && above > below;
  const maxHeight = Math.max(0, Math.min(360, up ? above : below));
  return { x, y, width, menuLeft, up, maxHeight };
}
