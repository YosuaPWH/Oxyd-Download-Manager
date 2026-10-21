# Oxyd Download Manager

Download manager desktop untuk Windows yang mirip IDM: file dipecah menjadi beberapa
segmen yang diunduh paralel, bisa di-pause/resume (juga setelah crash atau mati listrik),
dan terintegrasi dengan Chrome serta Edge lewat ekstensi + native messaging.

Dibangun dengan **Rust** (mesin download), **Tauri 2 + Svelte 5** (aplikasi desktop) dan
**ekstensi Manifest V3**. Spesifikasi lengkap ada di [docs/PRD.md](docs/PRD.md), rencana
pengerjaan di [docs/PLAN.md](docs/PLAN.md).

```
Ekstensi MV3 ──stdio──▶ oxyd-bridge.exe ──\\.\pipe\oxyd──▶ Oxyd.exe (Tauri)
(Chrome/Edge)  ◀──────                  ◀──────────────    ├─ UI (WebView2)
                                                           ├─ antrean
                                                           └─ oxyd-core ─ SQLite + disk
```

## Fitur

- Segmented download sampai 32 koneksi (default 8), dynamic segmentation (segmen sisa
  terbesar dibelah saat ada koneksi menganggur), fallback ke 1 koneksi bila server tidak
  mendukung Range.
- Pre-allocate file `.oxydpart`, tulis per offset, progres dicatat ke SQLite tiap detik
  **setelah** `fsync` sehingga resume tidak pernah menghasilkan file korup.
- Retry dengan exponential backoff, `Retry-After`, timeout baca 30 detik. Saat server
  membalas 429/503 koneksi langsung dikurangi, lalu ditambah lagi satu per satu setelah
  server tenang.
- Saat internet putus atau server sibuk, download dicoba ulang otomatis (30 detik, berlipat
  sampai 10 menit) dan otomatis masuk antrean lagi saat aplikasi dibuka ulang.
- Validasi ETag/Last-Modified/ukuran saat resume.
- **Perbarui link seperti IDM**: pilih *Ambil link baru dari browser* pada download yang
  berhenti atau link-nya kedaluwarsa, buka lagi halaman download-nya dan klik link-nya. Oxyd
  memakai link baru itu (beserta cookie sesi yang baru) untuk download yang sama dan
  melanjutkan dari progres terakhir. URL baru juga bisa ditempel manual.
- Link dari browser yang nama dan ukurannya sama dengan download yang belum selesai
  (termasuk yang masih mencoba ulang) ditawarkan untuk **dilanjutkan**, bukan diunduh ulang
  sebagai `file (1)`. Kalau dialog konfirmasi dimatikan, download itu langsung dilanjutkan.
- Antrean dengan batas download bersamaan, urutan bisa diubah, speed limit global dan per
  download (bisa diubah saat berjalan), jadwal mulai/jeda, aksi setelah antrean selesai
  (sleep/shutdown).
- Kategori otomatis (Video, Musik, Dokumen, Arsip, Program, Lainnya) dengan opsi subfolder,
  penanganan nama duplikat (rename/timpa/tanya), checksum MD5/SHA-256 opsional, proxy
  HTTP/SOCKS5.
- Ekstensi: tangkap download, kirim cookie (termasuk HttpOnly), Referer dan User-Agent,
  filter jenis file/ukuran/domain, menu klik kanan **Download dengan Oxyd**, `Alt+Shift+O`
  untuk melewatkan satu download, fallback ke browser bila aplikasi tidak menjawab dalam 3
  detik.
- Pencarian (Ctrl+F), bersihkan daftar download selesai (file tetap ada), checksum opsional
  di dialog download baru.
- System tray, notifikasi Windows, mulai bersama Windows, pemantau clipboard, tema
  terang/gelap, Bahasa Indonesia/Inggris (termasuk pesan error).
- Cookie dan header otorisasi disimpan terenkripsi DPAPI (per user Windows) dan dihapus
  setelah download selesai; URL di log dipotong tanpa query/token.

## Struktur

| Folder | Isi |
|---|---|
| `crates/oxyd-core` | Mesin download: probe, segmentasi, worker, limiter, SQLite, manager/antrean |
| `crates/oxyd-cli` | CLI `oxyd-cli` untuk menguji engine |
| `crates/oxyd-protocol` | Tipe pesan JSON, framing 4 byte LE, server named pipe |
| `crates/oxyd-bridge` | Native messaging host `oxyd-bridge.exe` |
| `app/` | Aplikasi Tauri (`src-tauri/`) dan UI Svelte (`src/`) |
| `extension/` | Ekstensi Chrome/Edge (TypeScript, esbuild) |
| `installer/` | Hook NSIS dan template manifest native host |
| `test-server/` | Server HTTP lokal untuk pengujian (range, throttle, putus koneksi, 429/503, ETag) |
| `scripts/` | Build installer, registrasi dev, uji kill/resume |

## Kebutuhan

- Windows 10/11 64-bit, WebView2 (sudah ada di Windows 11)
- Rust stable (MSVC), Node.js 22+

## Menjalankan saat development

```bash
# test semua crate Rust
cargo test --workspace

# CLI
cargo run -p test-server                      # http://127.0.0.1:8787
cargo run -p oxyd-cli -- download "http://127.0.0.1:8787/file/104857600?rate=1048576" -c 8
cargo run -p oxyd-cli -- list

# aplikasi desktop (UI + backend, hot reload)
cd app && npm install && npx tauri dev

# UI saja di browser dengan backend palsu
cd app && npm run dev
```

Saat aplikasi berjalan ia otomatis mendaftarkan `oxyd-bridge.exe` yang ada di folder yang
sama sebagai native host untuk Chrome dan Edge. Untuk mendaftarkan bridge secara manual:

```bash
powershell -ExecutionPolicy Bypass -File scripts/register-dev.ps1
```

### Memasang ekstensi (load unpacked)

```bash
cd extension && npm install && npm run build
```

Buka `chrome://extensions` atau `edge://extensions`, aktifkan **Developer mode**, klik
**Load unpacked** dan pilih folder `extension/build`. ID ekstensi selalu
`gnihamodjbioaennpopmldmjdecbmfpl` karena dipatok lewat field `key` di manifest; ID ini
yang diizinkan di `allowed_origins` native host. Popup ekstensi menampilkan status koneksi
ke aplikasi.

Uji bridge tanpa browser:

```bash
cargo run -p oxyd-bridge --example browser_sim -- ping
cargo run -p oxyd-bridge --example browser_sim -- https://example.com/file.zip
```

## Membuat installer

```bash
powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1
```

Hasilnya `target/release/bundle/nsis/Oxyd Download Manager_<versi>_x64-setup.exe`.
Installer memasang aplikasi dan `oxyd-bridge.exe` per user, menulis manifest native host ke
`%LOCALAPPDATA%\Oxyd\native-host\` beserta registry key Chrome dan Edge di HKCU, dan
menghapus semuanya saat uninstall.

## Data dan log

| Lokasi | Isi |
|---|---|
| `%LOCALAPPDATA%\Oxyd\oxyd.db` | Database download, segmen, pengaturan |
| `%LOCALAPPDATA%\Oxyd\logs\` | `oxyd.log.*` (aplikasi), `bridge.log` |
| `%LOCALAPPDATA%\Oxyd\native-host\` | Manifest native messaging |

`oxyd.db` berskema v3 (migrasi otomatis lewat `PRAGMA user_version`).
`OXYD_DATA_DIR` mengganti folder data (mode portable/pengujian), `OXYD_LOG` mengatur level
log (mis. `debug`), `OXYD_PIPE` mengganti nama pipe.

## Performa

Benchmark terhadap server uji lokal (release build, Ryzen 5 220, NVMe):

```bash
cargo run --release -p oxyd-core --example bench -- <folder sementara> [filter]
```

| Skenario | Sebelum optimasi | Sesudah |
|---|---|---|
| 64 MiB, 8 koneksi dengan kecepatan acak 0,5–4 MiB/s | 6,1–7,0 dtk | 4,4–4,6 dtk |
| 60 file × 512 KiB, latensi 30 ms | 1,87 dtk | 0,94 dtk |
| 64 MiB, 8 koneksi × 2 MiB/s (ideal 4,0 dtk) | 4,03 dtk | 4,03 dtk |
| Tulisan pertama di offset 1,75 GiB file 2 GiB (NTFS) | 1,06 dtk tertahan | tanpa jeda (file sparse) |

Yang membuatnya lebih cepat:

- **Segmentasi sadar kecepatan**: koneksi yang menganggur membantu segmen yang paling lama
  selesai (bukan yang sisa byte-nya terbesar), dan koneksi lambat hanya menyimpan porsi
  sebanding kecepatannya.
- **Probe sekaligus download**: respons probe (`Range: bytes=0-`) langsung menjadi koneksi
  segmen pertama, jadi file kecil cukup satu request. Koneksi probe dari dialog dikembalikan
  ke pool sehingga download berikutnya tidak perlu handshake TCP/TLS lagi.
- **File `.oxydpart` sparse**: NTFS tidak lagi mengisi nol seluruh file sebelum segmen di
  belakang bisa menulis, dan setiap byte tidak ditulis dua kali ke disk.
- **Baca jaringan dan tulis disk bertumpuk**: satu blok ditulis sambil blok berikutnya diterima.
- **Throttle cerdas**: saat server membalas 429/503 koneksi dikurangi saat itu juga, lalu
  ditambah lagi bertahap.

Pemakaian memori aplikasi (working set):

| Kondisi | Sebelum | Sesudah |
|---|---|---|
| Berjalan di tray (`--background`, autostart, dibuka bridge) | 367 MB (7 proses) | 21 MB (1 proses) |
| Setelah jendela ditutup | 391 MB | 35 MB |
| Jendela terbuka | ±370–390 MB | ±370–390 MB (WebView2) |

Jendela baru dibuat saat dibuka dan dihancurkan saat ditutup; membukanya lagi ±0,1–0,2 dtk.

## Pengujian ketahanan

```bash
powershell -ExecutionPolicy Bypass -File scripts/kill-resume-test.ps1 -Runs 100 -SizeMB 64
```

Skrip ini mematikan proses CLI secara paksa (`taskkill /F`) di saat acak selama download,
melanjutkannya lagi, lalu memverifikasi isi file byte per byte.

## Lisensi

MIT OR Apache-2.0
