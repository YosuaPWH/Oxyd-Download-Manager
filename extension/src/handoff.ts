// Moves a browser download to Oxyd without losing it when Oxyd cannot take it.
//
// The browser download is only paused while Oxyd is asked. If Oxyd accepts, the browser
// copy is cancelled; otherwise the paused download simply resumes in the browser, which
// keeps one-time links and POST downloads working (re-requesting them would fail).

export interface DownloadOps {
  pause(id: number): Promise<void>;
  resume(id: number): Promise<void>;
  cancel(id: number): Promise<void>;
  erase(id: number): Promise<void>;
  /** Start the URL again in the browser (last resort). */
  redownload(url: string, filename?: string): Promise<void>;
  /** Ask Oxyd to take the download; true when accepted. */
  offer(): Promise<boolean>;
}

export type HandOffResult = "oxyd" | "browser" | "gone";

export async function handOffDownload(
  item: { id: number; url: string; filename?: string },
  ops: DownloadOps,
): Promise<HandOffResult> {
  try {
    await ops.pause(item.id);
  } catch {
    return "gone"; // finished or removed meanwhile: leave it alone
  }
  if (await ops.offer()) {
    try {
      await ops.cancel(item.id);
      await ops.erase(item.id);
    } catch {
      // Already gone; Oxyd has it anyway.
    }
    return "oxyd";
  }
  try {
    await ops.resume(item.id);
  } catch {
    // The paused download cannot continue (e.g. interrupted): fetch it again.
    await ops.redownload(item.url, item.filename);
  }
  return "browser";
}
