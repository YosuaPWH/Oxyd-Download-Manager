export interface ExtSettings {
  /** Capture downloads at all. */
  enabled: boolean;
  /** Only capture files at least this large (bytes) when the size is known. */
  minSize: number;
  /** File extensions to capture (lower case, without dot). Empty = everything. */
  extensions: string[];
  /** Domains whose downloads stay in the browser (sub domains included). */
  excludedDomains: string[];
  /** Show the "Download" button over videos. */
  videoButton: boolean;
}

export const DEFAULT_EXTENSIONS = [
  "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz", "zst", "iso", "img", "bin", "cab",
  "exe", "msi", "msix", "apk", "dmg", "deb", "rpm", "jar",
  "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp",
  "mp3", "wav", "flac", "aac", "ogg", "m4a", "wma", "opus",
  "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "epub",
];

export const DEFAULTS: ExtSettings = {
  enabled: true,
  minSize: 1024 * 1024,
  extensions: DEFAULT_EXTENSIONS,
  excludedDomains: [],
  videoButton: true,
};

export async function loadSettings(): Promise<ExtSettings> {
  const stored = await chrome.storage.sync.get({ ...DEFAULTS } as Record<string, unknown>);
  return { ...DEFAULTS, ...(stored as Partial<ExtSettings>) };
}

export async function saveSettings(s: Partial<ExtSettings>): Promise<void> {
  await chrome.storage.sync.set(s);
}

/** Split user input ("zip, .rar  mp4") into normalised tokens. */
export function parseList(text: string, stripDot = false): string[] {
  return [
    ...new Set(
      text
        .split(/[\s,;]+/)
        .map((s) => s.trim().toLowerCase())
        .map((s) => (stripDot ? s.replace(/^\./, "") : s))
        .filter(Boolean),
    ),
  ];
}
