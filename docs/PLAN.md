# Rencana Implementasi Oxyd Download Manager

Dokumen ini menjabarkan langkah pengerjaan Oxyd berdasarkan [PRD.md](PRD.md). Setiap fase menghasilkan sesuatu yang bisa dijalankan dan diuji, sehingga progres terasa sejak awal.

**Asumsi:** dikerjakan sendiri, paruh waktu (±10–15 jam/minggu), target Windows 10/11.

---

## Ringkasan Fase

| Fase | Fokus | Hasil | Estimasi |
|---|---|---|---|
| 0 | Persiapan | Workspace, tooling, CI | 2–3 hari |
| 1 | Core: download dasar | CLI download satu koneksi | 1 minggu |
| 2 | Core: segmented download | CLI download paralel | 1–2 minggu |
| 3 | Core: persistensi & resume | Pause/resume tahan crash | 1–2 minggu |
| 4 | Aplikasi Tauri | UI daftar download + tray | 2 minggu |
| 5 | Bridge native messaging | `oxyd-bridge.exe` + named pipe | 1 minggu |
| 6 | Ekstensi Chrome/Edge | Tangkap download dari browser | 1–2 minggu |
| 7 | Installer & registrasi | Installer NSIS siap pakai | 1 minggu |
| 8 | Stabilisasi MVP (v0.1) | Bug fix, pengujian end-to-end | 1–2 minggu |
| 9 | v0.2 | Fitur P1 | 3–4 minggu |
| 10 | v0.3 | Fitur P2 | 3–4 minggu |

Total MVP (fase 0–8): sekitar **10–14 minggu**.

---

## Struktur Repository

```
oxyd-download-manager/
├── Cargo.toml                 # workspace
├── crates/
│   ├── oxyd-core/              # mesin download (library)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── probe.rs       # HEAD/range check, parsing header
│   │   │   ├── segment.rs     # pembagian & dynamic split
│   │   │   ├── worker.rs      # task download per segmen
│   │   │   ├── writer.rs      # penulisan file per offset
│   │   │   ├── task.rs        # state machine satu download
│   │   │   ├── queue.rs       # antrean & batas concurrency
│   │   │   ├── limiter.rs     # speed limiter
│   │   │   ├── store.rs       # SQLite
│   │   │   ├── events.rs      # event progres ke UI
│   │   │   └── error.rs
│   │   └── tests/
│   ├── oxyd-cli/               # CLI untuk testing core
│   ├── oxyd-protocol/          # tipe pesan bersama (serde)
│   └── oxyd-bridge/            # native messaging host
├── app/                       # aplikasi Tauri
│   ├── src-tauri/
│   └── src/                   # frontend (Svelte/TS)
├── extension/
│   ├── manifest.json
│   ├── src/
│   │   ├── background.ts
│   │   ├── native.ts
│   │   ├── filters.ts
│   │   └── popup/
│   └── build/
├── installer/
│   ├── native-host/com.oxyd.bridge.json.template
│   └── nsis-hooks.nsh
├── test-server/               # server lokal untuk pengujian
└── docs/
    ├── PRD.md
    └── PLAN.md
```

## Dependensi Utama

| Crate / Paket | Kegunaan |
|---|---|
| `tokio` | Runtime async |
| `reqwest` (fitur `stream`, `rustls-tls`, `cookies`, `socks`) | HTTP client |
| `futures` | Stream body response |
| `rusqlite` (fitur `bundled`) atau `sqlx` | Database lokal |
| `serde`, `serde_json` | Serialisasi |
| `thiserror`, `anyhow` | Error handling |
| `tracing`, `tracing-subscriber`, `tracing-appender` | Logging ke file |
| `uuid` | ID download |
| `governor` | Speed limiter (token bucket) |
| `interprocess` | Named pipe |
| `content_disposition` / `mime_guess` | Nama file dan kategori |
| `directories` | Lokasi folder AppData |
| `indicatif`, `clap` | CLI |
| `tauri` v2 + `tauri-plugin-single-instance`, `-notification`, `-autostart`, `-dialog`, `-opener` | Aplikasi desktop |
| `axum` (dev) | Server uji lokal |
| `@types/chrome`, `esbuild` atau `vite` | Build ekstensi |

---

## Fase 0 — Persiapan

**Tugas**
- [ ] Install Rust stable, `cargo-tauri`, Node.js LTS, WebView2 (biasanya sudah ada di Windows 11).
- [ ] Buat Cargo workspace dengan crate `oxyd-core`, `oxyd-cli`, `oxyd-protocol`, `oxyd-bridge`.
- [ ] Setup `rustfmt`, `clippy`, `.editorconfig`.
- [ ] GitHub Actions: `cargo fmt --check`, `cargo clippy`, `cargo test` di `windows-latest`.
- [ ] Buat `test-server` (axum) yang bisa: melayani file dengan/ tanpa dukungan range, membatasi kecepatan per koneksi, memutus koneksi secara acak, mengembalikan 429/503, mengubah ETag.

**Selesai jika:** `cargo test` hijau di CI dan test server bisa dijalankan lokal.

---

## Fase 1 — Core: Download Dasar

**Tugas**
- [ ] `probe.rs`: HEAD request, fallback ke `GET Range: bytes=0-0` jika HEAD ditolak. Hasil: ukuran, `accept_ranges`, ETag, Last-Modified, nama file, final URL.
- [ ] Penentuan nama file: `Content-Disposition` → path URL → fallback `download.bin`. Sanitasi karakter ilegal Windows (`<>:"/\|?*`) dan nama terlarang (`CON`, `NUL`, dll.).
- [ ] Download satu koneksi dengan streaming ke file `.oxydpart`, buffer 64–256 KB.
- [ ] Event progres via `tokio::sync::watch` (byte terunduh, kecepatan rata-rata bergerak, ETA).
- [ ] Mendukung header kustom (Cookie, Referer, User-Agent).
- [ ] `oxyd-cli download <url> [-o path]` dengan progress bar.

**Selesai jika:** CLI bisa mengunduh file 1 GB dari test server dan hash-nya cocok.

---

## Fase 2 — Core: Segmented Download

**Tugas**
- [ ] `segment.rs`: bagi ukuran file ke N segmen; segmen minimum 1 MB agar file kecil tidak dipecah berlebihan.
- [ ] `writer.rs`: pre-allocate file (`set_len`), tulis per offset. Di Windows gunakan `FileExt::seek_write`; satu file handle dibagi lewat `Arc` atau satu task writer yang menerima `(offset, bytes)` dari channel.
- [ ] `worker.rs`: satu task per segmen, request `Range: bytes=start-end`, validasi respons `206` dan `Content-Range`. Jika server membalas `200`, batalkan mode multi-segmen dan fallback ke satu koneksi.
- [ ] Retry per segmen: exponential backoff (1s, 2s, 4s, … maks 30s), maks 10 percobaan, timeout baca 30s.
- [ ] Cancellation dengan `tokio_util::sync::CancellationToken`.
- [ ] Agregasi progres seluruh segmen.
- [ ] Opsi `--connections N` di CLI.

**Pengujian**
- [ ] Test server dengan batas 1 MB/s per koneksi: 8 koneksi harus mendekati 8 MB/s.
- [ ] Server tanpa dukungan range: fallback berjalan.
- [ ] Putus koneksi acak: semua segmen tetap selesai, hash cocok.

**Selesai jika:** download paralel konsisten lebih cepat dan hasil selalu identik dengan file asli.

---

## Fase 3 — Core: Persistensi & Resume

**Tugas**
- [ ] `store.rs`: skema SQLite `downloads`, `segments`, `settings` (lihat PRD §9.4) dengan migrasi sederhana (`user_version`).
- [ ] Simpan progres segmen setiap 1 detik dan saat pause/stop. Gunakan WAL mode.
- [ ] Setelah menulis data, `flush` sebelum mencatat progres, agar progres yang tercatat tidak pernah melebihi data di disk.
- [ ] `task.rs`: state machine `Queued → Probing → Downloading ⇄ Paused → Completed / Failed / Cancelled`.
- [ ] Resume: probe ulang, bandingkan ETag/Last-Modified/ukuran. Jika berubah, tandai `failed` dengan alasan "file di server berubah" dan tawarkan mulai ulang.
- [ ] Rename `.oxydpart` → nama akhir saat selesai; tangani nama bentrok (`file (1).zip`).
- [ ] `queue.rs`: batas download aktif, mulai otomatis item berikutnya.
- [ ] CLI: `oxyd-cli list`, `pause`, `resume` untuk pengujian.

**Pengujian**
- [ ] Kill proses (`taskkill /F`) di tengah download 100 kali secara acak, lalu resume: hash selalu cocok.
- [ ] Ubah ETag di server di antara pause dan resume: terdeteksi.

**Selesai jika:** tidak ada satu pun file korup pada pengujian kill/resume.

---

## Fase 4 — Aplikasi Tauri

**Tugas**
- [ ] Inisialisasi Tauri 2 di `app/` dengan frontend Svelte + TypeScript (atau pilihan lain sesuai PRD §14).
- [ ] `src-tauri` menjalankan `oxyd-core` di runtime tokio Tauri; state dibagikan via `tauri::State`.
- [ ] Commands: `add_download`, `start`, `pause`, `resume`, `cancel`, `remove`, `open_file`, `open_folder`, `get_downloads`, `get_settings`, `set_settings`.
- [ ] Event progres dikirim ke frontend dengan throttle (maks 4x/detik per download) agar UI tetap ringan.
- [ ] UI: daftar download, toolbar (Tambah URL, Mulai, Pause, Hapus), filter kategori di sidebar.
- [ ] Dialog "Download baru": URL, nama file, ukuran (hasil probe), folder tujuan, tombol Mulai / Nanti / Batal.
- [ ] Halaman Pengaturan (folder default, koneksi, batas bersamaan, autostart).
- [ ] System tray: buka jendela, pause semua, keluar. Tutup jendela = sembunyikan ke tray.
- [ ] `tauri-plugin-single-instance`: instance kedua meneruskan argumen ke instance pertama lalu keluar.
- [ ] Logging ke `%LOCALAPPDATA%\Oxyd\logs`.

**Selesai jika:** semua alur download bisa dilakukan dari UI dengan menempelkan URL secara manual.

---

## Fase 5 — Bridge Native Messaging

**Tugas**
- [ ] `oxyd-protocol`: enum pesan (`AddDownload`, `Ping`, `Pong`, `Ack`) dengan `serde` dan field versi `v`.
- [ ] `oxyd-bridge`:
  - [ ] Baca stdin: 4 byte panjang (u32 little-endian) lalu JSON. Tolak pesan > 1 MB.
  - [ ] Tulis stdout dengan format yang sama. Pastikan stdout dalam mode biner dan **tidak ada `println!` lain** yang mengotori stdout (logging hanya ke file).
  - [ ] Hubungkan ke named pipe `\\.\pipe\oxyd`. Jika gagal, jalankan `Oxyd.exe --background`, retry hingga 5 detik.
  - [ ] Teruskan pesan dua arah; keluar saat stdin ditutup.
- [ ] Di aplikasi: server named pipe menggunakan `interprocess`, dibatasi untuk user saat ini, memproses `AddDownload` dengan membuka dialog dan membalas `Ack`.
- [ ] Tool uji kecil yang mensimulasikan browser (menulis pesan berprefix panjang ke stdin bridge).

**Selesai jika:** pesan dari tool uji muncul sebagai dialog di aplikasi, baik saat aplikasi sudah berjalan maupun belum.

---

## Fase 6 — Ekstensi Chrome/Edge

**Tugas**
- [ ] `manifest.json` MV3: `background.service_worker`, permission `downloads`, `nativeMessaging`, `cookies`, `contextMenus`, `storage`, `host_permissions: ["<all_urls>"]`.
- [ ] Generate key pair dan isi field `key` agar ID ekstensi tetap selama development; catat ID-nya untuk `allowed_origins`.
- [ ] `background.ts`:
  - [ ] `chrome.downloads.onCreated` → cek filter (ekstensi file, ukuran minimum, domain dikecualikan, toggle aktif).
  - [ ] Jika cocok: `chrome.downloads.cancel` lalu `erase`.
  - [ ] Kumpulkan cookie dengan `chrome.cookies.getAll({ url })`, Referer dari `downloadItem.referrer`, User-Agent dari `navigator.userAgent`.
  - [ ] `chrome.runtime.sendNativeMessage("com.oxyd.bridge", msg)` dengan timeout 3 detik.
  - [ ] Fallback: jika gagal, `chrome.downloads.download({ url })` dan tandai URL di set sementara agar tidak ditangkap ulang.
- [ ] Menu klik kanan "Download dengan Oxyd" untuk link.
- [ ] Popup: status koneksi (via `ping`), toggle aktif, pengaturan filter (disimpan di `chrome.storage.sync`).
- [ ] Build dengan esbuild/vite ke folder `extension/build`.
- [ ] Uji di Chrome (`chrome://extensions`) dan Edge (`edge://extensions`) dengan Load unpacked.

**Catatan**
- Event `onCreated` muncul setelah browser mulai mengunduh; ini normal dan juga cara kerja ekstensi download manager lain.
- Hindari menangkap URL `blob:` dan `data:` karena tidak bisa diunduh ulang dari luar browser; biarkan browser yang menanganinya.

**Selesai jika:** klik link download di Chrome dan Edge memunculkan dialog Oxyd, dan file dari situs yang butuh login berhasil diunduh.

---

## Fase 7 — Installer & Registrasi

**Tugas**
- [ ] Konfigurasi bundler Tauri untuk NSIS; sertakan `oxyd-bridge.exe` sebagai resource/sidecar.
- [ ] Hook NSIS saat instal:
  - [ ] Tulis manifest `com.oxyd.bridge.json` ke `%LOCALAPPDATA%\Oxyd\native-host\` dengan `path` absolut ke `oxyd-bridge.exe`.
  - [ ] Tulis registry `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.oxyd.bridge` dan `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.oxyd.bridge` (nilai default = path manifest).
- [ ] Hook saat uninstall: hapus key dan manifest.
- [ ] Alternatif untuk development: skrip PowerShell `scripts/register-dev.ps1` yang melakukan registrasi yang sama.
- [ ] Bootstrapper WebView2 diaktifkan.

**Contoh manifest native host**
```json
{
  "name": "com.oxyd.bridge",
  "description": "Oxyd native messaging bridge",
  "path": "C:\\Users\\<user>\\AppData\\Local\\Programs\\Oxyd\\oxyd-bridge.exe",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://<ID_DEV>/",
    "chrome-extension://<ID_CHROME_STORE>/",
    "chrome-extension://<ID_EDGE_STORE>/"
  ]
}
```

**Selesai jika:** di Windows bersih (VM), instal → pasang ekstensi → klik download langsung bekerja; uninstall membersihkan semuanya.

---

## Fase 8 — Stabilisasi MVP (v0.1)

**Tugas**
- [ ] Uji end-to-end dengan situs nyata: file publik besar, Google Drive, GitHub Releases, situs dengan login, link bertoken.
- [ ] Ukur metrik PRD §11: kecepatan vs browser, RAM idle, CPU.
- [ ] Penanganan error yang jelas di UI (403, 404, 410, disk penuh, path tidak bisa ditulis).
- [ ] Cek disk kosong sebelum pre-allocate.
- [ ] Hapus cookie dari database setelah download selesai.
- [ ] README instalasi dan penggunaan.
- [ ] Tag rilis `v0.1.0`, unggah installer ke GitHub Releases.

**Selesai jika:** semua kebutuhan P0 di PRD terpenuhi dan metrik tercapai.

---

## Fase 9 — v0.2 (Fitur P1)

- [ ] Dynamic segmentation: saat worker selesai, ambil segmen aktif dengan sisa terbesar (di atas ambang, misal 2 MB) dan belah dua; worker baru mengambil separuh belakang.
- [ ] Penyesuaian koneksi otomatis saat sering menerima 429/503.
- [ ] Speed limiter global dan per download (`governor`), bisa diubah saat download berjalan.
- [ ] Refresh link: input URL baru, probe, validasi ukuran sama, lanjutkan progres.
- [ ] Kategori otomatis dan folder per kategori.
- [ ] Notifikasi Windows.
- [ ] Visualisasi segmen di jendela detail.
- [ ] Ekstensi: daftar domain dikecualikan, status di popup.
- [ ] Mengurutkan antrean dan prioritas.

## Fase 10 — v0.3 (Fitur P2)

- [ ] Penjadwal (mulai/berhenti jam tertentu) dan aksi setelah selesai (sleep/shutdown).
- [ ] Pemantau clipboard.
- [ ] Verifikasi checksum.
- [ ] Proxy HTTP/SOCKS5.
- [ ] Tema gelap/terang, bahasa Indonesia/Inggris.
- [ ] Tombol pintas untuk melewati penangkapan di ekstensi.

---

## Strategi Pengujian

| Level | Cakupan | Alat |
|---|---|---|
| Unit | Pembagian segmen, parsing header, sanitasi nama file, state machine | `cargo test` |
| Integrasi | Download vs test server (range/non-range, putus koneksi, 429, ETag berubah, throttle) | `cargo test` + `test-server` |
| Ketahanan | Kill proses acak lalu resume, verifikasi hash | Skrip PowerShell |
| Protokol | Encode/decode pesan native messaging, pesan rusak/terlalu besar | `cargo test` di `oxyd-bridge` |
| End-to-end | Browser → ekstensi → bridge → app → file selesai | Manual + checklist di Fase 8 |
| Performa | Throughput, RAM, CPU | Task Manager / `perfmon`, skrip benchmark |

## Urutan Pengerjaan Minggu Pertama

1. Hari 1: workspace Cargo, CI, kerangka `test-server`.
2. Hari 2–3: `probe.rs` + sanitasi nama file + unit test.
3. Hari 4–5: download satu koneksi + CLI dengan progress bar.
4. Akhir minggu: uji unduh file 1 GB dari test server, cocokkan hash.

## Keputusan yang Perlu Diambil Sebelum Fase 4

- Framework frontend (Svelte, Solid, atau vanilla TS).
- Dialog konfirmasi selalu muncul atau ada mode langsung mulai.
- Nama final aplikasi (memengaruhi nama host native messaging dan path instalasi).
