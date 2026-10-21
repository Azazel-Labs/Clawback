# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · Türkçe · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback, disk alanınızı nelerin kullandığını gösterir. Bir sürücüyü veya klasörü tarar ve dosyaları, klasörlerinin içinde kapladıkları alanla orantılı dikdörtgenlerle gösterir. Windows, macOS ve Linux için masaüstü uygulaması ve terminal modu sunar.

![Clawback](../images/desktop.png)

*Ekran görüntülerinde yerleşik bir demo sürücüsü kullanılmıştır; dosya adları ve boyutları kurgusaldır.*

## Özellikler

Boyuta göre sıralanmış ağacı ve dosya türü dağılımını inceleyin. Yakınlaştırmak için çift tıklayın, dönmek için Geri veya Escape kullanın. Harita tarama sırasında kullanılabilir; duraklatma, sürdürme ve canlı güncellemeler desteklenir. Dosyaları açın, özelliklerini görün veya çöp kutusuna taşıyın; silme devre dışı bırakılabilir. On renk paleti, ayarlanabilir yoğunluk ve boyut hesaplama, 31 dilde arayüz.

![Clawback](../images/explore.png)

## Kurulum

En son sürümü indirin. Windows'ta `clawback.exe` çalıştırın, macOS'ta `Clawback.app` uygulamasını Applications klasörüne sürükleyin, Linux'ta `clawback` çalıştırın. **Dosya → Klasör aç** seçeneğini kullanın. crates.io üzerinden kurulum Rust 1.95 veya yenisini gerektirir ve sistemdeki Çince, Japonca ve Korece yazı tiplerini kullanır; indirilebilir sürümler bunları içerir.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Aşağıdaki komutlar masaüstü uygulamasını, etkileşimli terminal haritasını ve metin raporunu açar. Terminal, gezinmeyi ve canlı güncellemeleri destekler ancak dosya işlemlerini desteklemez. Raporlar betiklerde de kullanılabilir.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Tarama

Windows'ta Turbo, UAC onayı isteyen ayrı ve salt okunur bir yardımcı işlemle yerel NTFS biriminin tamamını daha hızlı okur. Normal tarama kullanılabilir durumda kalır. macOS, yönetici yetkisi olmadan meta verileri toplu okur. Dizin tarayıcısı iş parçacığı sayısını otomatik ayarlar. Okunamayan klasörler bildirilir; sembolik bağlantılar ve Windows birleşim noktaları izlenmez.

## Katkıda bulunma

Derleme yönergeleri ve kullanıcı kılavuzu İngilizcedir. Çeviriler Git'te tutulur; CI, eksiksiz kataloglar ve güncel İngilizce kaynaklarla karşılaştırılarak incelenmiş README dosyaları gerektirir.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
