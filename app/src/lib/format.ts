const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function bytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return "—";
  let v = n;
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return u === 0 ? `${n} B` : `${v.toFixed(v < 10 ? 2 : 1)} ${UNITS[u]}`;
}

export function speed(n: number): string {
  return n > 0 ? `${bytes(n)}/s` : "";
}

export function duration(secs: number | null | undefined): string {
  if (secs === null || secs === undefined) return "";
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m ${secs % 60}s`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h ${m % 60}m`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

/** Seconds until `unix`, never negative. */
export function until(unix: number, now: number): number {
  return Math.max(0, unix - now);
}

export function percent(done: number, total: number | null): number | null {
  if (!total) return null;
  return Math.min(100, (done / total) * 100);
}

export function date(unix: number | null): string {
  if (!unix) return "";
  return new Date(unix * 1000).toLocaleString();
}

/** Parse "1.5 MB", "500k", "2m" into bytes per second; empty means no limit. */
export function parseRate(text: string): number | null {
  const m = text.trim().toLowerCase().match(/^([\d.]+)\s*([kmg]?)b?(\/s)?$/);
  if (!m) return null;
  const n = parseFloat(m[1]);
  const mult = { "": 1024, k: 1024, m: 1024 ** 2, g: 1024 ** 3 }[m[2] as "" | "k" | "m" | "g"];
  const v = Math.round(n * mult);
  return v > 0 ? v : null;
}

export function rateInput(n: number | null): string {
  if (!n) return "";
  if (n % 1024 ** 2 === 0) return `${n / 1024 ** 2} MB`;
  return `${Math.round(n / 1024)} KB`;
}
