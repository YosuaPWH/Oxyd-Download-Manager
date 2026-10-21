# Improvements

What was measured, fixed and added after the first working version of Oxyd, and what is
still open. The [README](../README.md) describes the app itself; this file is the changelog
of *why it is faster, safer and nicer to use*.

- [How the numbers were measured](#how-the-numbers-were-measured)
- [Download speed](#download-speed)
- [Memory and CPU](#memory-and-cpu)
- [Reliability](#reliability)
- [Privacy and security](#privacy-and-security)
- [Workflow and interface](#workflow-and-interface)
- [Browser extension](#browser-extension)
- [Tooling](#tooling)
- [Known limitations and ideas](#known-limitations-and-ideas)

## How the numbers were measured

Throughput numbers come from `crates/oxyd-core/examples/bench.rs`, which downloads from the
local `test-server` (release build, Ryzen 5 220, NVMe, Windows 11):

```bash
cargo run --release -p oxyd-core --example bench -- <scratch folder> [scenario filter]
# OXYD_BENCH_SERVER=http://127.0.0.1:8797 uses a test server in another process
```

| Scenario | What it models |
|---|---|
| `local-2GiB-8conn`, `local-512MiB-1conn` | Disk and engine overhead with an unlimited server |
| `throttled-64MiB-2MiBps-8conn` | A server that caps every connection (ideal: 4.0 s) |
| `jitter-64MiB-2MiBps-8conn` | Connections of uneven speed (0.5–4 MiB/s each), typical for mirrors and CDNs |
| `small-60x512KiB-30ms` | Many small files with 30 ms request latency |

> **Caveat.** These are loopback measurements: no packet loss, no congestion control and
> almost no latency. They show that an optimisation works under controlled conditions, not
> how much faster a real download from the internet will be. A comparison against real
> mirrors is still to do (see [limitations](#known-limitations-and-ideas)).

Two bugs in the test rig itself were fixed first, otherwise the results would have measured
the rig: the server's per-chunk sleeps were rounded up by the Windows timer (halving the
configured rate), and its byte generator did a division per byte, capping it at about
480 MiB/s.

## Download speed

| Change | Before → after |
|---|---|
| **Speed-aware dynamic segmentation.** An idle connection used to split the segment with the most bytes left in half, so slow connections became the tail of the download. Now the segment that will take longest *by measured speed* is split, and the slow connection keeps only a share proportional to its speed. | 64 MiB over 8 connections of random speed: **6.1–7.0 s → 4.4 s** (evenly throttled downloads are unchanged at the ideal 4.0 s) |
| **The probe response becomes the first segment.** New downloads probe with `Range: bytes=0-` and read segment 0 from that same response instead of opening a second request (servers without Range even sent the whole body twice). The plain probe used by the dialog reads its single byte so the connection returns to the pool and skips the next TCP/TLS handshake. | 60 files × 512 KiB, 30 ms latency: **1.87 s → 0.94 s**; a small download now needs exactly one request |
| **Sparse part files on Windows.** Segments write far into a freshly sized file; NTFS then zero-fills everything before each write, so every byte hits the disk twice. `.oxydpart` files are created sparse and turned into regular files on completion. | One 64 KiB write at 1.75 GiB into a 2 GiB file: **1.06 s stall → none** on NVMe (much worse on hard disks) |
| **Network reads overlap disk writes.** A segment keeps receiving while its previous 256 KiB block is written; progress is still recorded only after the write finished, so it never runs ahead of the data on disk. The limiter is skipped when no speed limit is set. | One connection against a local server: 437 → about 450 MiB/s (small on NVMe; slower disks gain more) |
| **Throttling response.** A 429/503 used to be handled only after a worker finished. A throttled worker now gives up its connection immediately (the last one keeps retrying with backoff) and connections come back one at a time after 10 quiet seconds. | Against a server that allows 2 connections: **24 → 6** refused requests |

Tried and not kept: a 1 MiB write block instead of 256 KiB (no measurable difference, more
memory). Considered and not done: HTTP/2, which multiplexes all requests onto one TCP
connection and so works against parallel connections (the HTTP client is built without it).

## Memory and CPU

The hidden main window kept six WebView2 processes alive even when Oxyd only sat in the
tray. The window is now created on demand (always on the main thread, because links arrive
on the pipe thread) and destroyed when closed. Opening it again takes 0.1–0.2 s, from the
tray, from a second launch, or because a link arrived from the browser.

| State | Before | After |
|---|---|---|
| Tray / `--background` (autostart, started by the bridge) | 367 MB, 7 processes | **21 MB**, 1 process (PRD target: < 60 MB) |
| After closing the window | 391 MB | **35 MB** |
| Window open | about 370–390 MB (WebView2) | unchanged |

Idle CPU is about 0 %. While a download runs in the tray, UI events are no longer
serialised for a window that does not exist; the CPU difference is within noise (1–2 % of
total CPU either way).

Smaller changes: the scheduler, *pause all*, *resume all* and the duplicate-name check use
indexed status queries (schema v3) instead of loading the whole history, moving a queue item
re-emits only the rows whose position changed, and the UI re-sorts its list only when the
queue order changes.

## Reliability

| Problem | Fix |
|---|---|
| *Pause all* with "after queue: shut down / sleep" turned the computer off, because an emptied queue counted as finished | The after-queue action only runs when the last download completed or failed on its own |
| Queue order and per-download speed limits changed during a download were reverted when it paused or finished (the task saved its old copy of the record) | Task saves skip those columns; the speed limit is a targeted update |
| A zero-byte response without `Content-Length` crashed the worker (`start + 0 - 1` overflow) | The segment list is emptied and the file completes as empty |
| A network outage failed the download for good within seconds (PRD US-05) | Temporary failures (network, 408/429/5xx, exhausted retries) are retried with backoff: 30 s doubling up to 10 minutes, at most 30 times, and re-queued when the app restarts. Pause all suspends them |
| A single reset connection during the first request failed the download | The probe is retried after 250 ms and 1 s. Found as a flaky test: 2 failures in 16 runs under load, 0 in 60 afterwards |
| A *remove* that raced with completion was ignored | The completed outcome now honours the stop request |
| Network errors only said "error sending request" | The cause chain is kept ("… connection closed before message completed"), without the URL |
| Clicking a new link for a stopped download created a duplicate `file (1)` and left the old progress behind | See [workflow](#workflow-and-interface) |

Verification: `scripts/kill-resume-test.ps1` kills the CLI with `taskkill /F` at random
moments during a throttled 64 MB download and resumes it until it finishes; the file is then
compared byte by byte. 100 kills, file intact (also after every later engine change). Rust
tests cover segmented downloads, range and length fallbacks, dropped connections, 503/429
handling, redirects, pause/resume, restarts, changed ETags, queue limits, speed limits,
checksums, automatic retries and more.

## Privacy and security

- **Cookies and `Authorization` are encrypted at rest** with DPAPI (per Windows user) and
  removed when a download completes. Older plain values are still read.
- **No tokens in logs.** Signed download links carry access tokens in the query string, so
  logged URLs are reduced to scheme, host and path; `reqwest` error messages are stripped of
  the URL before they reach the log or the database.
- The bridge pipe only accepts local clients and its DACL grants access to the owner and
  SYSTEM only; the native messaging host manifest lists the extension ID in
  `allowed_origins`; the bridge refuses frames above 1 MB.
- Incognito downloads send the incognito cookie store, not the normal profile's cookies.

## Workflow and interface

- **Refresh a link like IDM.** Choose *Get a new link from the browser* on a stopped or
  expired download, open the page again and click the download link once. Oxyd uses that
  link, with the browser's fresh cookies, for the *same* download and continues from the last
  byte. It works for downloads that are still retrying their dead link, and a pending wait
  expires after 10 minutes.
- **Matching unfinished downloads.** A link whose file name (ignoring a ` (n)` duplicate
  counter) and size match an unfinished download is offered as *Continue that one* in the
  New download dialog; with the dialog turned off, a match with a known size continues
  automatically instead of becoming `file (1)`. Found and fixed end to end: when the
  connection drops, the old download is usually still retrying, so running downloads match
  too.
- Refreshing through the browser replaces the old cookies and referer
  (`Manager::refresh_link`); pasting a URL by hand into the detail panel keeps the old ones.
- **Search** (Ctrl+F) by file name or URL, **Clear** in the Finished view (files stay on
  disk), and an optional **checksum** (`sha256:` / `md5:`) in the New download dialog.
- **Readable errors.** Failures show a translated message (English or Indonesian) for the
  error code with the raw details underneath, and automatic retries show a countdown.
- The toolbar no longer wraps; narrow windows show icon-only buttons with tooltips. The tray
  menu follows the language setting.
- The schedule's *start* time resumes only what its *stop* time paused, plus downloads added
  with "Later"; downloads you paused by hand stay paused.

## Browser extension

- **Pause, then hand off.** A browser download used to be cancelled before Oxyd answered and
  restarted from scratch when Oxyd was unreachable, which breaks one-time links and POST
  downloads. It is now paused, cancelled only after Oxyd accepts, and resumed otherwise.
- The "skip next download" switch and the bypass list live in `chrome.storage.session` with
  an alarm for the badge, because an MV3 service worker forgets module variables when it is
  stopped.
- The extension and its tests (`npm test`, 10 tests for the filters and the hand-off) run in CI.

## Tooling

- `crates/oxyd-core/examples/bench.rs` plus test-server options: `rate`, `jitter`, `latency`,
  `maxconn` (answers 429 beyond N connections), `cookie` (a fake login session), `drop`,
  `err`, `status`, `etag`, `range=0`, `nolen=1`, and request counters.
- `crates/oxyd-bridge/examples/browser_sim.rs` plays the browser, including cookies and size
  (`OXYD_SIM_COOKIES`, `OXYD_SIM_SIZE`), for end-to-end tests against a release build.
- CI: Rust format, clippy and tests on Windows, extension tests, app type check and build,
  and a workflow that builds the installer on every push to `main`.
- Release profile with thin LTO, one codegen unit and stripped binaries; the installer is
  about 4.8 MB.

## Known limitations and ideas

Not verified yet:

- Benchmarks against real mirrors and CDNs (only loopback so far).
- The extension in a real Chrome and Edge, and the installer on a clean Windows VM
  (including uninstall removing the registry keys).
- Real-world sites: login-protected downloads, Google Drive, GitHub Releases, tokenised links.

Ideas, roughly by value:

- Code signing for the installer and the bridge (SmartScreen and antivirus warnings).
- Auto-update and a `v0.1.0` release with the installer attached.
- Virtualised download list for very long histories.
- Segment-level resume across a changed ETag when the server serves identical bytes.
- Per-server connection limits learned from earlier 429s.
- Video streams (HLS/DASH), macOS and Linux, Firefox: out of scope for v1 (see the PRD).
