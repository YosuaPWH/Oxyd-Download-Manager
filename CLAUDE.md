# Oxyd

A download manager for Windows and Linux that stays small. This file is the standing
direction for whoever works on the code next: what Oxyd is for, the rules its design follows,
how to check a change, and what is known to be unfinished.

## What it is for

Oxyd takes over downloads from the browser, fetches them as fast as the server allows and
never loses progress: a download survives a pause, a crash, a dead link and a restart. It
covers what people use a download manager for (files, videos from pages, torrents, FTP,
batches, queues) in one light app instead of a suite.

Three things decide a trade-off, in this order:

1. **Never lose or corrupt a download.** Progress is recorded only after the bytes are on
   disk. A feature that risks a wrong file is not shipped.
2. **Stay small.** The release binary is about 28 MB and compresses to about 8 MB; the
   ceiling is 50 MB for an installer. In the tray the app must idle at about 0% CPU: no
   polling loops, wake on events. A dependency has to earn its size.
3. **One calm window.** Colour only on what moves; every figure in the mono face. Extra
   windows (progress, mini) are opt-out and each costs a web view, so nothing else gets its
   own window without a reason.

## Layout

- `crates/oxyd-core`: everything that is not UI. `manager/` owns the queue and dispatches a
  download by kind: HTTP (`manager/mod.rs`, segmented), video pages (`manager/media.rs`),
  FTP (`manager/ftp.rs`), torrents (`manager/torrent.rs`, behind the `torrent` feature, which
  only the app enables), tool jobs (`manager/tools.rs`). New kinds plug in at
  `Manager::run_inner` and reuse the part file, `ticker` and checkpoints where they can.
- `app/src-tauri`: the Tauri shell. Keep it thin: commands call the core, logic that can be
  tested goes into the core.
- `app/src`: Svelte 5 UI. One bundle, three roots chosen in `main.ts` by the query string:
  the main window, `?mini`, `?progress=<id>`. Tokens live in `app.css`; components use the
  tokens, never raw colours. `lib/mock.ts` is a fake backend for `npm run dev` in a browser.
- `extension`: the MV3 extension. `content.ts` is the in-page video button.
- `test-server`: the local HTTP server the integration tests download from.

## Rules that came from mistakes

These were real bugs here or in Prism, the project Oxyd's layout follows. Do not undo them.

- Cookies and credentials go only to the origin they were given for
  (`RequestHeaders::for_target`). A redirect to another site gets none.
- A part file stays sparse until the download completes. Never clear the flag on pause.
- A window or dialog that failed to open is retried, not marked as handled
  (`AppState::progress_wanted`, `syncPending` in the store). One the user closed stays closed.
- The video button is never drawn in fullscreen, and a dismissed video stays dismissed when
  the page redraws it (`extension/src/dismiss.ts`).
- Subscribe to events before loading the first snapshot.
- The after-queue action (sleep, shutdown) only follows a queue that finished by itself.

## Checking a change

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd app && npm run check && npm test && npm run build)
(cd extension && npm run typecheck && npm test && npm run build)
```

The Tauri crate needs `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev` on
Linux (`apt-get update` first) and an existing `app/dist` folder.

A fix for a bug comes with a test that fails without it. UI changes are looked at in a
browser through the mock (`npm run dev`, then `?lang=en`, `?mini`, `?progress=a`), in both
themes. Say plainly what was only compiled and not run.

Tests that need something the machine may lack skip themselves and say so: the media tool
tests without `ffmpeg`, the torrent peer test without IPv6 sockets.

## Known gaps

Honest state, most important first:

- **Never run for real:** the torrent download between two sessions (no IPv6 in the
  container it was written in), and the Tauri windows themselves (progress, mini, dropping
  links): they compile and their views were checked against the mock only.
- **Not measured since torrents were added:** the Windows installer size and tray memory.
- Torrents download every file (no selection) and do not seed after completion.
- FTP is one connection and has no FTPS.
- The grabber reads plain HTML; links added by scripts are not seen.
- The extension is Chromium only.

## Next

In this order, because each removes more risk than the one after it:

1. Run the app on Windows and Linux once: torrent, progress window, mini window, drop.
   Fix what that shows before adding anything.
2. Measure installer size and tray memory again and put the numbers in the README.
3. File selection for torrents.
4. Firefox build of the extension.

## Conventions

- Commits are authored by the repository owner and carry no `Co-Authored-By` line.
- English in code, comments and the README; the UI is Indonesian and English
  (`app/src/lib/i18n.svelte.ts`, both languages in the same change).
- Comments say why, not what. No version is bumped and no tag is pushed without being asked:
  a tag publishes a release.
