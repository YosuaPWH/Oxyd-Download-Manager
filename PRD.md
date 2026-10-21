# PRD: RustDM — Download Manager Desktop Berbasis Rust

| | |
|---|---|
| **Status** | Draft v0.1 |
| **Tanggal** | 21 Oktober 2026 |
| **Platform awal** | Windows 10/11 (64-bit) |
| **Browser** | Google Chrome, Microsoft Edge (Chromium) |
| **Nama** | RustDM (nama sementara) |

---

## 1. Ringkasan

RustDM adalah aplikasi download manager desktop yang mirip Internet Download Manager (IDM). Aplikasi ini mempercepat download dengan memecah file menjadi beberapa segmen yang diunduh paralel, mendukung pause/resume, dan terintegrasi dengan Chrome serta Edge melalui ekstensi browser sehingga download otomatis ditangkap oleh aplikasi.

Aplikasi dibangun dengan **Rust** (mesin download dan backend), **Tauri** (antarmuka desktop), dan **ekstensi Manifest V3** yang terhubung lewat **Native Messaging**.

## 2. Latar Belakang dan Masalah

- Download bawaan browser hanya memakai satu koneksi, sehingga lambat pada server yang membatasi kecepatan per koneksi.
- Download browser sering gagal di tengah jalan dan tidak selalu bisa dilanjutkan.
- Tidak ada antrean, penjadwalan, atau pembatas kecepatan yang memadai di browser.
- IDM berbayar, hanya untuk Windows, dan tertutup (closed source).

## 3. Tujuan

1. Download lebih cepat daripada browser untuk server yang mendukung HTTP Range, melalui segmented download paralel.
2. Download tahan gangguan: bisa di-pause, di-resume, dan otomatis melanjutkan setelah koneksi putus atau aplikasi ditutup.
3. Integrasi mulus dengan Chrome dan Edge: klik link download di browser langsung tertangkap oleh RustDM.
4. Aplikasi ringan: memakai sumber daya kecil saat berjalan di background.

## 4. Non-Goals (Bukan Cakupan v1)

- Dukungan macOS dan Linux (dirancang agar bisa diport, tapi tidak dirilis di v1).
- Firefox, Safari, dan browser non-Chromium.
- Download video streaming (HLS/DASH), BitTorrent, dan FTP.
- Sinkronisasi cloud, akun pengguna, atau aplikasi mobile.
- Melewati DRM, paywall, atau proteksi situs.

## 5. Target Pengguna

- **Pengguna umum** yang sering mengunduh file besar (installer, video, arsip) dan menginginkan download yang lebih cepat dan bisa dilanjutkan.
- **Pengguna dengan koneksi tidak stabil** yang butuh resume otomatis.
- **Pengguna teknis** yang menginginkan alternatif IDM yang ringan dan open source.

## 6. User Stories

| ID | Sebagai... | Saya ingin... | Supaya... |
|---|---|---|---|
| US-01 | Pengguna | klik link download di Chrome/Edge dan otomatis ditangkap RustDM | tidak perlu menyalin link secara manual |
| US-02 | Pengguna | melihat dialog konfirmasi berisi nama file, ukuran, dan folder tujuan | bisa mengubah lokasi simpan sebelum mulai |
| US-03 | Pengguna | download berjalan dengan banyak koneksi paralel | download selesai lebih cepat |
| US-04 | Pengguna | pause dan resume download kapan saja | bisa mengatur pemakaian bandwidth |
| US-05 | Pengguna | download melanjutkan sendiri setelah internet putus atau laptop restart | tidak perlu mengulang dari awal |
| US-06 | Pengguna | menempelkan URL secara manual ke aplikasi | bisa mengunduh link dari sumber lain |
| US-07 | Pengguna | mengunduh file dari situs yang butuh login | download tidak gagal karena sesi tidak terbawa |
| US-08 | Pengguna | mengatur antrean dan jumlah download bersamaan | koneksi tidak kewalahan |
| US-09 | Pengguna | membatasi kecepatan download | tetap bisa browsing atau video call |
| US-10 | Pengguna | file otomatis dikelompokkan ke folder sesuai jenisnya | folder Downloads tetap rapi |
| US-11 | Pengguna | menekan tombol pintas agar satu download tidak ditangkap RustDM | file kecil tetap diunduh browser |
| US-12 | Pengguna | memperbarui link yang kedaluwarsa tanpa kehilangan progres | download dari link bertoken tetap bisa dilanjutkan |

## 7. Kebutuhan Fungsional

### 7.1 Mesin Download (Core)

| ID | Kebutuhan | Prioritas |
|---|---|---|
| F-CORE-01 | Probe URL (HEAD atau GET `Range: bytes=0-0`) untuk membaca ukuran, dukungan range, ETag, Last-Modified, dan nama file dari `Content-Disposition` | P0 |
| F-CORE-02 | Segmented download dengan jumlah koneksi yang bisa diatur (default 8, maks 32) | P0 |
| F-CORE-03 | Fallback ke satu koneksi jika server tidak mendukung range atau ukuran tidak diketahui | P0 |
| F-CORE-04 | Pre-allocate file dan tulis setiap segmen langsung ke offset-nya | P0 |
| F-CORE-05 | Dynamic segmentation: membelah segmen yang tersisa paling besar saat ada koneksi menganggur | P1 |
| F-CORE-06 | Retry otomatis dengan exponential backoff, timeout per koneksi, dan penanganan 429/503 | P0 |
| F-CORE-07 | Follow redirect dan mendukung HTTPS | P0 |
| F-CORE-08 | Mengirim header kustom: Cookie, Referer, User-Agent, Authorization | P0 |
| F-CORE-09 | Verifikasi integritas saat resume dengan membandingkan ETag/Last-Modified | P0 |
| F-CORE-10 | Speed limiter global dan per download (token bucket) | P1 |
| F-CORE-11 | Verifikasi checksum (MD5/SHA-256) opsional setelah selesai | P2 |
| F-CORE-12 | Dukungan proxy HTTP/SOCKS5 | P2 |

### 7.2 Persistensi dan Resume

| ID | Kebutuhan | Prioritas |
|---|---|---|
| F-PERS-01 | Menyimpan state setiap download (URL, header, path, ukuran, progres per segmen, ETag) ke SQLite | P0 |
| F-PERS-02 | State disimpan berkala (misalnya tiap 1 detik) dan saat pause/keluar | P0 |
| F-PERS-03 | Saat aplikasi dibuka kembali, download yang belum selesai dipulihkan | P0 |
| F-PERS-04 | Fitur "Refresh link": mengganti URL download tanpa menghapus progres, setelah validasi ukuran sama | P1 |
| F-PERS-05 | File sementara memakai ekstensi `.rdmpart` dan di-rename setelah selesai | P0 |

### 7.3 Antrean dan Penjadwalan

| ID | Kebutuhan | Prioritas |
|---|---|---|
| F-QUEUE-01 | Batas jumlah download aktif bersamaan (default 3) | P0 |
| F-QUEUE-02 | Mengubah urutan antrean (naik/turun, prioritas) | P1 |
| F-QUEUE-03 | Penjadwalan mulai dan berhenti pada jam tertentu | P2 |
| F-QUEUE-04 | Aksi setelah antrean selesai: tidak ada, sleep, atau shutdown | P2 |

### 7.4 Integrasi Browser (Ekstensi Chrome/Edge)

| ID | Kebutuhan | Prioritas |
|---|---|---|
| F-EXT-01 | Satu kode ekstensi Manifest V3 yang berjalan di Chrome dan Edge | P0 |
| F-EXT-02 | Menangkap download lewat `chrome.downloads.onCreated`, membatalkan download browser, dan meneruskan ke RustDM | P0 |
| F-EXT-03 | Mengirim URL, nama file, ukuran, MIME, Referer, User-Agent, dan cookie domain terkait | P0 |
| F-EXT-04 | Filter berdasarkan ekstensi file dan ukuran minimum (bisa diatur di popup ekstensi) | P0 |
| F-EXT-05 | Fallback: jika RustDM tidak merespons dalam 3 detik, download dibiarkan di browser | P0 |
| F-EXT-06 | Menu klik kanan "Download dengan RustDM" pada link | P1 |
| F-EXT-07 | Tombol on/off penangkapan dan daftar domain yang dikecualikan | P1 |
| F-EXT-08 | Popup ekstensi menampilkan status koneksi ke aplikasi | P1 |
| F-EXT-09 | Tombol pintas untuk melewati penangkapan pada satu download | P2 |

### 7.5 Bridge (Native Messaging Host)

| ID | Kebutuhan | Prioritas |
|---|---|---|
| F-BRG-01 | Executable terpisah (`rdm-bridge.exe`) yang berkomunikasi dengan ekstensi melalui stdin/stdout (pesan JSON dengan prefix panjang 4 byte little-endian) | P0 |
| F-BRG-02 | Meneruskan pesan ke aplikasi utama melalui named pipe `\\.\pipe\rustdm` | P0 |
| F-BRG-03 | Menjalankan aplikasi utama jika belum berjalan, lalu mencoba ulang koneksi | P0 |
| F-BRG-04 | Membalas ekstensi dengan status `accepted`, `rejected`, atau `error` | P0 |
| F-BRG-05 | Validasi ukuran pesan (maks 1 MB dari browser) dan skema JSON | P0 |

### 7.6 Aplikasi Desktop (UI)

| ID | Kebutuhan | Prioritas |
|---|---|---|
| F-UI-01 | Jendela utama: daftar download dengan nama, ukuran, progres, kecepatan, ETA, status | P0 |
| F-UI-02 | Dialog "Download baru" saat link diterima dari ekstensi atau ditempel manual | P0 |
| F-UI-03 | Aksi per item: mulai, pause, resume, batal, hapus, buka file, buka folder, salin URL | P0 |
| F-UI-04 | Visualisasi segmen (bar per koneksi) di jendela detail download | P1 |
| F-UI-05 | System tray: aplikasi tetap berjalan saat jendela ditutup | P0 |
| F-UI-06 | Notifikasi Windows saat download selesai atau gagal | P1 |
| F-UI-07 | Kategori otomatis (Video, Musik, Dokumen, Arsip, Program, Lainnya) dengan folder tujuan masing-masing | P1 |
| F-UI-08 | Halaman pengaturan: folder default, jumlah koneksi, batas download bersamaan, speed limit, mulai bersama Windows | P0 |
| F-UI-09 | Pemantau clipboard: menawarkan download saat URL file disalin | P2 |
| F-UI-10 | Tema terang dan gelap mengikuti sistem | P2 |
| F-UI-11 | Penanganan nama file duplikat (rename otomatis, timpa, atau tanya) | P0 |

### 7.7 Instalasi

| ID | Kebutuhan | Prioritas |
|---|---|---|
| F-INST-01 | Installer Windows (NSIS via Tauri bundler) yang memasang aplikasi dan `rdm-bridge.exe` | P0 |
| F-INST-02 | Installer menulis manifest native host dan registry key untuk Chrome dan Edge (HKCU) | P0 |
| F-INST-03 | Uninstaller menghapus registry key dan manifest | P0 |
| F-INST-04 | Pilihan "Jalankan saat Windows mulai" | P1 |

## 8. Kebutuhan Non-Fungsional

| Kategori | Kebutuhan |
|---|---|
| **Performa** | Pada server yang mendukung range dan membatasi kecepatan per koneksi, throughput minimal 3x download browser. Pada koneksi tanpa batas per koneksi, minimal setara browser. |
| **Sumber daya** | RAM idle < 60 MB (aplikasi + WebView). CPU idle < 1%. Saat 3 download aktif, CPU < 10% pada laptop kelas menengah. |
| **Ukuran** | Installer < 15 MB. |
| **Keandalan** | Tidak ada file korup setelah pause/resume, crash, atau mati listrik (progres tersimpan, data diverifikasi). Download gagal tidak boleh menghilangkan file yang sudah selesai. |
| **Startup** | Aplikasi siap menerima link < 2 detik setelah dijalankan oleh bridge. |
| **Keamanan** | Native host hanya menerima ekstensi resmi melalui `allowed_origins`. Named pipe hanya bisa diakses user yang sama. Cookie tidak disimpan ke disk dalam bentuk plaintext setelah download selesai. Tidak membuka port jaringan. |
| **Privasi** | Tidak ada telemetri tanpa persetujuan eksplisit pengguna. |
| **Aksesibilitas** | Navigasi keyboard di jendela utama dan dialog. |
| **Lokalisasi** | Bahasa Indonesia dan Inggris. |

## 9. Arsitektur

```
┌──────────────────┐   stdio (JSON)   ┌──────────────┐   named pipe   ┌──────────────────────┐
│ Ekstensi MV3     │ ───────────────▶ │ rdm-bridge   │ ─────────────▶ │ Aplikasi Tauri       │
│ (Chrome / Edge)  │ ◀─────────────── │ (Rust)       │ ◀───────────── │  ├─ UI (WebView)     │
└──────────────────┘                  └──────────────┘                │  ├─ Queue manager    │
                                                                      │  └─ rdm-core engine  │
                                                                      │        │             │
                                                                      │     SQLite + Disk    │
                                                                      └──────────────────────┘
```

### 9.1 Komponen

| Komponen | Teknologi | Tanggung Jawab |
|---|---|---|
| `rdm-core` | Rust, tokio, reqwest, rusqlite | Probe, segmentasi, download, retry, persistensi, speed limit |
| `rdm-bridge` | Rust, interprocess, serde_json | Native messaging host, meneruskan pesan ke aplikasi |
| `app` | Tauri 2, TypeScript + framework UI ringan (Svelte/Solid) | UI, tray, notifikasi, server named pipe, queue manager |
| `extension` | TypeScript, Manifest V3 | Menangkap download, mengumpulkan cookie/header, UI popup |
| Installer | Tauri bundler (NSIS) + hook | Instalasi aplikasi, bridge, dan registrasi registry |

### 9.2 Protokol Pesan (Ekstensi ↔ Bridge ↔ App)

Semua pesan berupa JSON dengan field `v` (versi protokol) dan `type`.

**Ekstensi → App: menambah download**
```json
{
  "v": 1,
  "type": "add_download",
  "id": "uuid-dari-ekstensi",
  "url": "https://example.com/file.zip",
  "filename": "file.zip",
  "size": 104857600,
  "mime": "application/zip",
  "referrer": "https://example.com/page",
  "userAgent": "Mozilla/5.0 ...",
  "cookies": "name=value; name2=value2",
  "source": "chrome"
}
```

**App → Ekstensi: respons**
```json
{ "v": 1, "type": "ack", "id": "uuid-dari-ekstensi", "status": "accepted" }
```

**Ekstensi → App: cek status**
```json
{ "v": 1, "type": "ping" }
```
Dibalas `{ "v": 1, "type": "pong", "appVersion": "0.1.0" }`.

### 9.3 Registrasi Native Messaging (Windows)

- Nama host: `com.rustdm.bridge`
- Manifest: `%LOCALAPPDATA%\RustDM\native-host\com.rustdm.bridge.json`
- Registry Chrome: `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.rustdm.bridge`
- Registry Edge: `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.rustdm.bridge`
- `allowed_origins` berisi ID ekstensi Chrome Web Store, ID Edge Add-ons, dan ID development (dipatok dengan field `key` di `manifest.json`).

### 9.4 Model Data (SQLite)

**downloads**
| Kolom | Tipe | Keterangan |
|---|---|---|
| id | TEXT (UUID) | Primary key |
| url | TEXT | URL aktif |
| final_url | TEXT | URL setelah redirect |
| filename | TEXT | Nama file |
| save_path | TEXT | Path lengkap tujuan |
| total_size | INTEGER NULL | Null jika tidak diketahui |
| downloaded | INTEGER | Byte yang sudah diunduh |
| status | TEXT | queued, probing, downloading, paused, completed, failed, cancelled |
| resumable | INTEGER | 0/1 |
| etag | TEXT NULL | |
| last_modified | TEXT NULL | |
| headers_json | TEXT | Referer, User-Agent, cookie (dihapus setelah selesai) |
| category | TEXT | |
| connections | INTEGER | Jumlah koneksi yang diizinkan |
| speed_limit | INTEGER NULL | Byte/detik |
| error | TEXT NULL | Pesan error terakhir |
| created_at, updated_at, completed_at | INTEGER | Unix timestamp |

**segments**
| Kolom | Tipe | Keterangan |
|---|---|---|
| download_id | TEXT | FK ke downloads |
| idx | INTEGER | Urutan segmen |
| start | INTEGER | Offset awal (inklusif) |
| end | INTEGER | Offset akhir (inklusif) |
| downloaded | INTEGER | Byte terunduh di segmen ini |

**settings**: key-value untuk pengaturan aplikasi.

## 10. Alur Utama

### 10.1 Download dari Browser
1. Pengguna klik link download di Chrome/Edge.
2. Ekstensi menerima event `onCreated`, mencocokkan filter, membatalkan download browser.
3. Ekstensi mengambil cookie dan header, lalu mengirim `add_download` ke native host.
4. Browser menjalankan `rdm-bridge.exe`, bridge menghubungi aplikasi via named pipe (menjalankan aplikasi jika perlu).
5. Aplikasi menampilkan dialog "Download baru" dan membalas `ack: accepted`.
6. Pengguna klik Mulai, `rdm-core` melakukan probe lalu download tersegmentasi.
7. Selesai: file di-rename dari `.rdmpart`, notifikasi muncul, cookie dihapus dari database.

### 10.2 Fallback
Jika bridge atau aplikasi tidak merespons dalam 3 detik atau membalas `error`, ekstensi memulai ulang download di browser dengan `chrome.downloads.download()` dan menandai URL tersebut agar tidak ditangkap lagi.

## 11. Metrik Keberhasilan

- Peningkatan kecepatan rata-rata ≥ 3x dibanding browser pada server uji dengan batas per koneksi.
- ≥ 99% download dari browser berhasil ditangkap dan selesai tanpa intervensi manual (di luar kegagalan server).
- 0 kasus file korup pada pengujian pause/resume dan kill proses (100 percobaan).
- RAM idle < 60 MB.

## 12. Risiko dan Mitigasi

| Risiko | Dampak | Mitigasi |
|---|---|---|
| Situs memakai cookie HttpOnly atau token yang terikat sesi browser | Download gagal (403) | Kirim seluruh cookie via `chrome.cookies.getAll`, plus fallback ke browser |
| Link bertoken kedaluwarsa saat resume | Resume gagal | Fitur Refresh link, deteksi 403/410 dan beri tahu pengguna |
| Server membatasi jumlah koneksi atau memblokir banyak koneksi | Kecepatan turun atau error | Kurangi koneksi otomatis saat menerima 429/503 berulang |
| Perubahan kebijakan Manifest V3 | Fitur ekstensi rusak | Hanya pakai API stabil (`downloads`, `nativeMessaging`, `cookies`) |
| Antivirus menandai `rdm-bridge.exe` | Integrasi gagal | Code signing untuk rilis publik |
| Review Chrome Web Store/Edge Add-ons lama atau ditolak karena permission `<all_urls>` | Rilis tertunda | Justifikasi permission yang jelas, distribusi manual (load unpacked) untuk tahap awal |
| WebView2 tidak tersedia di Windows lama | Aplikasi tidak bisa jalan | Installer Tauri dengan bootstrapper WebView2 |

## 13. Rilis

| Versi | Cakupan |
|---|---|
| **v0.1 (MVP)** | Semua kebutuhan P0: core engine, persistensi, antrean dasar, ekstensi penangkap download, bridge, UI utama, tray, installer |
| **v0.2** | P1: dynamic segmentation, speed limiter, refresh link, kategori, notifikasi, menu klik kanan, visualisasi segmen |
| **v0.3** | P2: penjadwalan, clipboard monitor, checksum, proxy, tema |
| **Berikutnya** | Video (HLS/DASH), macOS/Linux, Firefox |

## 14. Pertanyaan Terbuka

1. Nama final aplikasi dan apakah akan open source (lisensi MIT/Apache-2.0?).
2. Framework UI frontend: Svelte, Solid, atau vanilla TypeScript?
3. Apakah dialog "Download baru" selalu muncul, atau ada mode "langsung mulai tanpa bertanya"?
4. Apakah perlu code signing sejak awal atau setelah rilis publik?
5. Distribusi ekstensi: lewat store resmi atau cukup load unpacked untuk penggunaan pribadi?
