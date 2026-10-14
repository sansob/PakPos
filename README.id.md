<p align="center">
  <img src="assets/icon-256.png" width="96" alt="Logo PakPos">
</p>

<h1 align="center">PakPos</h1>

<p align="center">
  API client yang langsung kebuka, datanya tetap di laptop kamu,<br>
  dan nggak minta login. Ditulis pakai Rust.
</p>

<p align="center">
  <a href="README.md">English</a> · <b>Bahasa Indonesia</b> · <a href="https://sansob.github.io/PakPos/">Dokumentasi</a>
</p>

![PakPos mode gelap](docs/screenshots/overview.png)

## Kenapa bikin ini?

Kebanyakan dari kita cuma mau kirim request, lihat JSON-nya, ganti satu header,
terus coba lagi. Tapi API client sekarang makin berat: aplikasi Electron yang
minta akun, sync collection ke cloud orang lain, dan butuh beberapa detik cuma
buat kebuka.

PakPos balik ke dasar dan berusaha ngerjain itu dengan baik. Aplikasinya native
(satu binary ~20 MB, tanpa Electron), kebukanya cepat, dan semua yang kamu bikin
disimpan sebagai file JSON biasa di folder pilihan kamu. Bagian terakhir ini
penting: taruh foldernya di repo git project kamu, dan satu tim langsung pakai
collection yang sama, perubahan di-review lewat pull request seperti kode biasa.

Namanya? *Pak Pos*, yang mengantar surat kamu dan bawa balasannya pulang. Ya
cuma itu tugasnya.

## Bisa apa saja sekarang

Kirim request dengan method apa pun, lengkap dengan query params, header, dan
body JSON, form, multipart, atau file. Auth Basic, Bearer, dan API key sudah ada,
dan request bisa "mewarisi" auth dari folder atau collection, jadi token cukup
diset sekali, bukan empat puluh kali.

Request disusun dalam collection dan folder. Environment menyimpan variabel
seperti `{{base_url}}` dan `{{token}}`: ganti dari Staging ke Production di
header, semua request ikut. Variabel yang ketemu warnanya hijau, yang nggak ketemu
merah, jadi typo langsung kelihatan.

Untuk test, PakPos nggak pakai script JavaScript, tapi pengecekan sederhana
seperti `status equals 200`, `json.data.id exists`, atau `time less than 500`.
Nilai dari response (misalnya `json.token`) juga bisa disimpan ke variabel buat
request berikutnya, dan itu sudah cukup untuk kebanyakan alur login. Karena test-nya
berupa data, bukan kode, menjalankan collection teman nggak akan menjalankan kode
mereka di laptop kamu.

Pindahan dari Postman? Export collection (v2.1) dan environment-nya, lalu import
di sini; sebagian besar langsung jalan. Bisa juga paste perintah `curl` langsung
ke kolom URL, atau sebaliknya pakai tombol `</>` untuk dapat snippet cURL,
Python, JavaScript, atau HTTPie.

Ada mode terang juga:

![PakPos mode terang](docs/screenshots/light.png)

## Command line dan CI

Mesin yang sama tersedia sebagai `pakpos-cli`, jadi collection yang kamu klik di
aplikasi bisa jalan juga di pipeline:

```sh
pakpos-cli run api/collection.json -e api/staging.json --var token=$API_TOKEN \
  --reporter junit --out report.xml
```

Exit code-nya bukan nol kalau ada test yang gagal, bisa output JUnit untuk GitHub
Actions/GitLab/Jenkins, dan bisa baca file Postman juga.

## Dipakai AI agent

`pakpos-cli mcp` menjalankan server [Model Context Protocol](https://modelcontextprotocol.io),
jadi Claude, Cursor, VS Code, dan sejenisnya bisa melihat collection kamu, menjalankan
request yang tersimpan, dan membaca hasilnya. Hati-hati: agent yang bisa kirim
HTTP request bisa menjangkau apa pun yang bisa dijangkau laptop kamu. Jadi jalankan
dengan `--allow-host` dan sebutkan hanya API yang boleh disentuh.

## Cara pasang

Ambil build untuk OS kamu di [halaman releases](https://github.com/sansob/PakPos/releases):
installer atau zip portable untuk Windows, `.dmg` untuk macOS (Apple Silicon dan
Intel jadi satu), dan `.deb` atau tarball untuk Linux.

Build-nya belum di-code-sign, jadi OS kamu akan protes saat pertama dibuka. Di
macOS klik kanan aplikasi lalu pilih **Open**; di Windows klik **More info → Run anyway**.

Mau build sendiri? Butuh Rust 1.95 atau lebih baru:

```sh
git clone https://github.com/sansob/PakPos && cd PakPos
cargo run --release -p pakpos
```

Saat pertama dibuka, otomatis ada collection demo supaya kamu bisa langsung
tekan **Send** (`Ctrl/⌘ + Enter`).

## Yang belum bisa

Proyek ini masih muda. Dibanding Postman, belum ada script JavaScript (yang dari
hasil import dilewati dengan peringatan), OAuth 2.0, request GraphQL/WebSocket/gRPC,
dan mock server. Sync cloud juga nggak ada, dan itu memang disengaja: pakai git saja.
Lihat [roadmap](docs/src/roadmap.md); semuanya boleh diambil kalau mau bantu.

## Mau bantu?

Issue, ide, perbaikan dokumentasi, dan terjemahan sama berharganya dengan kode.
Baca [CONTRIBUTING.md](CONTRIBUTING.md). Intinya: bikin branch dari `develop`,
pastikan `cargo fmt`, `clippy`, dan test lolos, lalu buka PR ke `develop`.

## Lisensi

MIT atau Apache-2.0, pilih yang cocok.

PakPos adalah proyek independen dan tidak berafiliasi dengan Postman, Inc.
