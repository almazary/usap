# Usap

**Jadikan HP Android trackpad untuk Mac, dengan rasa yang mendekati trackpad Mac.**

[English](README.en.md)

Usap terdiri dari dua bagian: aplikasi Android yang membaca posisi jari di layar HP, dan agen kecil di Mac
yang mengubah sentuhan itu menjadi gerak kursor, klik, scroll, dan gestur. Semua hitungan gestur
(akselerasi kursor, inersia scroll, pengenalan ketukan) dilakukan di Mac, jadi HP cukup mengirim posisi jari.

- Gerak kursor dengan akselerasi: geser pelan untuk presisi, geser cepat untuk jarak jauh.
- Scroll dua jari yang halus, dengan inersia setelah jari diangkat, memakai event scroll asli macOS
  sehingga aplikasi memperlakukannya seperti trackpad sungguhan.
- Ketuk untuk klik, ketuk dua jari untuk klik kanan, gestur tiga jari untuk Mission Control dan pindah desktop.
- Blok teks dan seret dengan cara yang mirip trackpad Mac.
- Empat jalur sambungan: kabel USB (tanpa mode developer), Bluetooth, Wi-Fi, dan USB debugging.
- Getaran di HP saat klik, dan indikator baterai HP di layar trackpad.

Agen ditulis dalam Rust, aplikasi Android dalam Kotlin tanpa library tambahan.

> Status: dipakai sehari-hari oleh pembuatnya, tapi belum banyak diuji di perangkat lain.
> Saat ini hanya untuk macOS. Dukungan Windows direncanakan.

## Kenapa Usap dibuat

Saya menghabiskan banyak waktu di depan laptop, dari jam 6 pagi sampai jam 12 malam. Tidak semuanya untuk
bekerja, tapi selama itu tangan saya hampir terus memegang mouse atau trackpad. Dulu saya memakai mouse dan
tidak ada masalah. Lalu saya terbiasa dengan trackpad Mac, dan ternyata jauh lebih nyaman. Saat kembali ke
mouse, tangan saya sering pegal. Menggerakkan mouse butuh tenaga lengan dan pergelangan, sedangkan trackpad
cukup digerakkan dengan jari.

Masalahnya, saya sering memakai keyboard eksternal. MacBook jadi tergeser ke belakang keyboard, trackpad-nya
tidak terjangkau, dan pasangan yang biasa dipakai untuk keyboard eksternal adalah mouse. HP Android hampir
selalu ada di meja, jadi Usap mengubah HP menjadi trackpad yang bisa ditaruh di samping keyboard. Tujuannya
terasa semirip mungkin dengan trackpad Mac: kursor yang makin jauh kalau digeser cepat, scroll yang masih
meluncur setelah jari diangkat, dan gestur yang sama.

## Kebutuhan

| | Minimal | Sudah diuji |
|---|---|---|
| Mac | macOS 11 Big Sur | macOS 26, Apple Silicon |
| HP | Android 8.0 (API 26) | TECNO CM5, Android 16 |

Untuk build dari kode sumber: Rust (stable) untuk agen, JDK 17 dan Android SDK 34 untuk aplikasi.

## Pasang

### 1. Agen di Mac

```sh
git clone https://github.com/almazary/usap.git
cd usap/agent
cargo build --release
cp target/release/usap-agent ~/.local/bin/   # atau folder lain di PATH Anda
```

### 2. Aplikasi di HP

Build APK, lalu pasang ke HP:

```sh
./build-apk.sh                       # hasilnya usap-<versi>-debug.apk
adb install -r usap-0.8-debug.apk
```

Tanpa `adb`, salin file APK ke HP lalu buka dari pengelola file. Android akan meminta izin memasang aplikasi
dari sumber tak dikenal.

Detail build ada di bagian [Build dari kode sumber](#build-dari-kode-sumber).

### 3. Jalankan pertama kali

```sh
usap-agent
```

macOS akan meminta beberapa izin untuk aplikasi terminal yang menjalankan agen (Terminal, iTerm, Alacritty, dan
sebagainya). Izinkan semuanya:

- **Accessibility** (Pengaturan Sistem → Privasi & Keamanan → Aksesibilitas): wajib, supaya agen boleh
  menggerakkan kursor. Setelah diizinkan, jalankan ulang agen.
- **Bluetooth**: untuk jalur Bluetooth.
- **Jaringan lokal**, dan firewall kalau menyala: untuk jalur Wi-Fi.

Agen menampilkan **kode 6 angka** di terminal. Kode ini disimpan di `~/.config/usap/code`, jadi tetap sama
setiap kali agen dijalankan. Buat kode baru dengan `usap-agent --new-code`.

Agen tidak perlu dijalankan dari folder proyek. Biarkan terminalnya terbuka selama memakai Usap.

## Menyambungkan HP

Pilih salah satu jalur. Angka telat di bawah ini hasil ukur di Mac dan HP pembuat.

| Jalur | Cara | Telat khas | Catatan |
|---|---|---|---|
| **Kabel USB** (Android Open Accessory) | Colok HP ke Mac. Usap terbuka sendiri dan langsung tersambung. | 2–3 ms | Paling akurat. Tidak perlu mode developer atau kode. |
| **Bluetooth LE** | Di HP tekan *Sambungkan lewat Bluetooth*. | sekitar 10 ms, stabil | Tanpa kabel. Bisa dipakai bersama keyboard Bluetooth. |
| **Wi-Fi** | Tekan *Cari komputer*, pilih Mac, masukkan kode. | 5 ms sampai ratusan ms | Sangat tergantung Wi-Fi. Lihat [Pemecahan masalah](#pemecahan-masalah). |
| **USB debugging** (adb) | Nyalakan USB debugging, colok, tekan *Kabel lewat USB debugging*. | 3–8 ms | Cadangan. Agen menjalankan `adb reverse tcp:50505 tcp:50505` sendiri. |

Jalur kabel USB perlu sedikit penjelasan. Agen mencari HP Android yang tercolok, lalu memintanya masuk mode
aksesori "Usap". Android menanyakan apakah Usap boleh dibuka untuk aksesori ini; centang *selalu* supaya tidak
ditanya lagi. Selama dalam mode ini, transfer file (MTP) HP tidak tersedia. Cabut kabel untuk kembali normal,
atau jalankan agen dengan `--no-aoa` kalau tidak ingin mode ini.

Di layar trackpad, tombol kembali di HP memutus sambungan.

## Gestur

| Gestur | Aksi |
|---|---|
| geser 1 jari | gerak kursor. Makin cepat digeser, makin jauh kursornya. |
| ketuk | klik. Ketuk dua kali untuk klik ganda, tiga kali untuk klik tiga kali. |
| tahan telunjuk diam, lalu geser jari tengah | blok teks atau seret (jendela, tab, file). HP bergetar saat klik mulai ditekan. Jari tengah boleh diangkat lalu ditaruh lagi untuk memblok lebih jauh. Angkat telunjuk untuk selesai. |
| geser 2 jari | scroll halus, dengan inersia setelah jari diangkat |
| ketuk 2 jari | klik kanan |
| 3 jari ke atas | Mission Control |
| 3 jari ke bawah | App Exposé |
| 3 jari ke kiri atau kanan | pindah desktop (Spaces) |

Gestur blok meniru cara di trackpad Mac: jempol menekan trackpad sampai berbunyi klik, lalu telunjuk menggeser.
Supaya tidak tertukar dengan scroll, klik baru ditekan kalau telunjuk sudah diam sebentar sebelum jari tengah
menempel, kedua jari berdekatan, dan telunjuk tidak ikut bergerak.

Dua gestur seret lain tersedia tapi mati secara bawaan, karena keduanya mudah terpicu tanpa sengaja:

- `hold_drag`: tahan satu jari diam sampai HP bergetar, lalu geser.
- `tap_drag`: ketuk, lalu segera tempelkan jari lagi dan geser.

Pindah desktop memakai shortcut Ctrl+← dan Ctrl+→, dan Mission Control memakai Ctrl+↑ dan Ctrl+↓. Kalau shortcut
ini Anda ubah di Pengaturan Sistem → Keyboard → Pintasan Papan Ketik, gestur tiga jari tidak akan bekerja.

## Pengaturan

Setelan ada di `~/.config/usap/config`. File ini dibuat otomatis saat agen pertama kali berjalan. Begitu
file disimpan, agen langsung memakai nilai barunya tanpa perlu dijalankan ulang.

```ini
# Kecepatan kursor. 1.0 = bawaan, 1.5 = lebih jauh, 0.7 = lebih pelan.
speed = 1.0
# Kecepatan scroll.
scroll = 1.0
# Arah scroll. true = seperti trackpad Mac (natural), false = terbalik.
natural = true
# Tekan dan seret satu jari: tahan jari diam selama sekian detik, lalu geser. 0 = mati.
hold_drag = 0
# Ketuk lalu segera tempel dan geser untuk menyeret.
tap_drag = false
```

Opsi baris perintah mengalahkan isi file:

| Opsi | Arti |
|---|---|
| `--speed <angka>` | kecepatan kursor |
| `--scroll <angka>` | kecepatan scroll |
| `--natural`, `--no-natural` | arah scroll |
| `--new-code` | buat kode sambung baru |
| `--port <port>` | port UDP untuk Wi-Fi, bawaan 50505 |
| `--buffer <ms>` | buffer penghalus Wi-Fi tetap. 0 = mati. Bawaan: otomatis. |
| `--no-ble` | jangan cari HP lewat Bluetooth |
| `--no-aoa` | jangan ajak HP yang tercolok masuk mode kabel |
| `--stats` | tampilkan kualitas sambungan setiap 2 detik |
| `--dry-run -v` | jangan gerakkan kursor, hanya tampilkan aksi |

## Pemecahan masalah

**Kursor tidak bergerak sama sekali.** Aplikasi terminal belum punya izin Accessibility, atau agen belum
dijalankan ulang setelah izin diberikan.

**Gerakan patah-patah lewat Wi-Fi.** Jalankan `usap-agent --stats` dan lihat angka `telat p90`. Kalau sering
di atas 30 ms, masalahnya di Wi-Fi, bukan di Usap. Penyebab yang umum:

- Mac tersambung ke Wi-Fi 2,4 GHz yang ramai. Pindah ke jaringan 5 GHz kalau router menyediakannya.
- AirDrop, Handoff, dan Sidecar memakai radio Wi-Fi yang sama dan bisa membuat jeda ratusan milidetik.

Agen sudah punya buffer yang menyesuaikan diri untuk meratakan paket yang datang bergerombol. Kalau Wi-Fi tetap
patah-patah, pakai kabel USB atau Bluetooth.

**Sentuhan tiba-tiba berhenti, atau `--stats` menunjukkan "dipotong Android".** Sistem HP mengambil alih
sentuhan untuk gestur bawaannya sendiri. Aplikasi tidak bisa mencegah ini sepenuhnya.

- Usap sudah melindungi bagian tengah tepi kiri dan kanan layar dari gestur kembali. Hindari mulai menggeser
  tepat dari tepi layar.
- Banyak HP punya gestur tiga jari bawaan, misalnya screenshot atau layar terpisah, yang bentrok dengan gestur
  tiga jari Usap. Matikan gestur itu di pengaturan HP, biasanya di menu Gestur atau Fitur khusus.

**Agen tidak menemukan HP lewat Bluetooth.** Pastikan aplikasi terminal punya izin Bluetooth di Pengaturan
Sistem → Privasi & Keamanan → Bluetooth, dan layar Usap di HP sedang terbuka setelah menekan
*Sambungkan lewat Bluetooth*.

**HP tidak bisa transfer file saat dicolok.** HP sedang dalam mode kabel Usap. Cabut kabelnya, atau jalankan
agen dengan `--no-aoa`.

## Keamanan dan privasi

- Jalur Wi-Fi dan Bluetooth meminta kode 6 angka dari agen. Lima kode salah dalam semenit mengunci sambungan
  baru selama 60 detik.
- Jalur kabel USB tidak meminta kode, karena HP terhubung langsung secara fisik ke Mac.
- Paket tidak dienkripsi. Isinya hanya posisi jari, tanpa teks atau isi layar, tapi tetap pakai jalur Wi-Fi
  hanya di jaringan yang Anda percaya.
- Usap tidak punya server dan tidak mengirim data ke internet. Aplikasi Android memang meminta izin
  `INTERNET`, tapi hanya karena Android mewajibkannya untuk jaringan lokal (jalur Wi-Fi).

## Cara kerja

```
HP (Kotlin)                                    Mac (Rust)
PadView: posisi jari dalam milimeter    ──►    sumber: UDP, TCP (adb), BLE, USB (AOA)
         + waktu sentuhan di HP                  │
                                                 ▼
                                              jitter.rs: meratakan paket yang datang bergerombol
                                                 │
                                                 ▼
                                              engine.rs: mesin gestur (murni, ada unit test)
                                                 │
                                                 ▼
                                              mac.rs: CGEvent (kursor, klik, scroll, shortcut)
getaran, baterai                        ◄──   balasan: HAPTIC, WELCOME
```

- **HP mengirim posisi mentah dalam milimeter**, bukan piksel, supaya terasa sama di HP mana pun.
  Semua keputusan gestur ada di `agent/src/engine.rs`, kode Rust murni tanpa API macOS, sehingga bisa dites.
- **Penghalus jaringan** (`agent/src/jitter.rs`) memakai waktu sentuhan dari HP untuk menjadwalkan ulang paket
  dengan jarak waktu yang benar, dengan buffer yang menyesuaikan diri (4–45 ms) berdasarkan keterlambatan terkini.
- **Scroll** dikirim sebagai event scroll kontinu dengan fase (mulai, berubah, selesai) dan fase inersia,
  seperti trackpad asli.
- **Bluetooth**: HP menjadi perangkat GATT (peripheral) dan agen menyambung sebagai central (`btleplug`).
  Beberapa paket digabung dalam satu notifikasi.
- **Kabel USB**: agen memakai `libusb` (lewat `rusb`) untuk mengalihkan HP ke mode Android Open Accessory, lalu
  membaca paket lewat bulk transfer.

Format paket dijelaskan di `agent/src/proto.rs`.

## Build dari kode sumber

### Agen

```sh
cd agent
cargo test             # tes mesin gestur dan protokol
cargo build --release  # hasilnya target/release/usap-agent
```

`libusb` ikut dikompilasi ke dalam binary, jadi tidak perlu Homebrew.

### Aplikasi Android

Pilih salah satu:

- **Android Studio**: buka folder `android/`, lalu Run.
- **Baris perintah**: `./build-apk.sh`. Butuh JDK 17 dan Android SDK 34 (`ANDROID_HOME`, atau `sdk.dir` di
  `android/local.properties`).
- **Di mesin lain lewat SSH dan Docker**: `USAP_BUILD_HOST=<host> ./build-apk.sh`. Mesin itu butuh image Docker
  berisi JDK 17 dan Android SDK 34 di `/opt/android-sdk`; namanya diatur dengan `USAP_BUILDER_IMAGE`. Setelan ini
  bisa disimpan di `build.local.env`, yang tidak ikut git.

Supaya APK baru bisa dipasang menimpa versi lama, build selalu memakai kunci debug yang sama. Untuk build lokal,
itu kunci debug bawaan Android Studio. Kunci lain bisa dipakai dengan menulis `usap.keystore=<path>` di
`android/local.properties`.

## Struktur proyek

```
agent/                 agen macOS (Rust)
  src/engine.rs        mesin gestur
  src/jitter.rs        penghalus jaringan
  src/mac.rs           keluaran ke macOS (CoreGraphics)
  src/proto.rs         format paket
  src/ble.rs           jalur Bluetooth
  src/aoa.rs           jalur kabel USB (Android Open Accessory)
  src/main.rs          sambungan, kode, setelan, statistik
android/               aplikasi Android (Kotlin)
  .../PadView.kt       layar trackpad
  .../Link.kt          jalur Wi-Fi dan USB debugging
  .../BleLink.kt       jalur Bluetooth
  .../AoaLink.kt       jalur kabel USB
build-apk.sh           build APK
```

## Rencana

- Agen untuk Windows.
- Pilihan gestur empat jari untuk HP yang memakai gestur tiga jari untuk fungsinya sendiri.
- File unduhan siap pakai (agen dan APK) di halaman Releases.

## Lisensi

[MIT](LICENSE)
