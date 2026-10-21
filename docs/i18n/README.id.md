# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · Bahasa Indonesia · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback menunjukkan apa yang menghabiskan ruang disk Anda. Aplikasi ini memindai drive atau folder dan menampilkan berkas sebagai persegi panjang sesuai ruang yang dipakai, di dalam foldernya. Tersedia untuk Windows, macOS, dan Linux, dengan aplikasi desktop dan mode terminal.

![Clawback](../images/desktop.png)

*Data demo - file dan ukuran fiktif*

## Fitur

Jelajahi pohon yang diurutkan berdasarkan ukuran dan rincian jenis berkas. Klik dua kali untuk memperbesar, lalu gunakan Kembali atau Escape untuk kembali. Peta dapat digunakan selama pemindaian, dengan jeda, lanjutkan, dan pembaruan langsung. Buka berkas, lihat properti, atau pindahkan ke tempat sampah; penghapusan dapat dinonaktifkan. Sepuluh palet warna, kepadatan dan perhitungan ukuran yang dapat diatur, serta antarmuka dalam 31 bahasa.

![Clawback](../images/explore.png)

## Instalasi

Unduh rilis terbaru. Di Windows jalankan `clawback.exe`, di macOS seret `Clawback.app` ke Applications, dan di Linux jalankan `clawback`. Pilih **Berkas → Buka folder**. Instalasi dari crates.io memerlukan Rust 1.95 atau lebih baru dan memakai font Tionghoa, Jepang, dan Korea dari sistem; rilis unduhan menyertakan font tersebut.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Perintah berikut membuka aplikasi desktop, peta terminal interaktif, dan laporan teks. Terminal mendukung navigasi dan pembaruan langsung, tetapi tidak mendukung tindakan pada berkas. Laporan juga dapat digunakan dalam skrip.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Pemindaian

Di Windows, Turbo membaca seluruh volume NTFS lokal dengan lebih cepat melalui proses pembantu terpisah yang hanya membaca dan meminta persetujuan UAC. Pemindaian biasa tetap tersedia. macOS membaca metadata secara berkelompok tanpa hak administrator. Pemindai direktori menyesuaikan jumlah utas secara otomatis. Folder yang tidak dapat dibaca dilaporkan; tautan simbolis dan junction Windows tidak diikuti.

## Kontribusi

Petunjuk kompilasi dan panduan pengguna tersedia dalam bahasa Inggris. Terjemahan dikelola di Git; CI mewajibkan katalog lengkap dan README yang ditinjau berdasarkan sumber bahasa Inggris terbaru.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
