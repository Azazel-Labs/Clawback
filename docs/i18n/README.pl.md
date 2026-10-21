# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · Polski · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback pokazuje, co zajmuje miejsce na dysku. Skanuje dysk lub folder i przedstawia pliki jako prostokąty proporcjonalne do zajętej przestrzeni, wewnątrz ich folderów. Działa w Windows, macOS i Linuksie, z interfejsem graficznym i trybem terminalowym.

![Clawback](../images/desktop.png)

*Dane demonstracyjne - fikcyjne pliki i rozmiary*

## Funkcje

Przeglądaj drzewo według rozmiaru i zestawienie typów plików. Podwójne kliknięcie przybliża, a Wstecz lub Escape cofa. Mapa działa podczas skanowania, z pauzą, wznowieniem i aktualizacjami na żywo. Otwieraj pliki, sprawdzaj właściwości lub przenoś do kosza; usuwanie można wyłączyć. Dziesięć palet, regulowana gęstość i sposób liczenia rozmiarów oraz interfejs w 31 językach.

![Clawback](../images/explore.png)

## Instalacja

Pobierz najnowsze wydanie. W Windows uruchom `clawback.exe`, w macOS przeciągnij `Clawback.app` do Applications, a w Linuksie uruchom `clawback`. Wybierz **Plik → Otwórz folder**. Instalacja z crates.io wymaga Rust 1.95 lub nowszego i korzysta z systemowych czcionek chińskich, japońskich i koreańskich; gotowe wydania zawierają własne.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Poniższe polecenia uruchamiają aplikację graficzną, interaktywną mapę w terminalu i raport tekstowy. Terminal obsługuje nawigację i aktualizacje na żywo, ale nie operacje na plikach. Raporty działają też w skryptach.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Skanowanie

W Windows Turbo szybciej odczytuje cały lokalny wolumin NTFS przez osobny proces pomocniczy tylko do odczytu, wymagający zgody UAC. Zwykłe skanowanie pozostaje dostępne. macOS odczytuje metadane partiami bez uprawnień administratora. Skaner katalogów automatycznie dobiera liczbę wątków. Nieczytelne foldery są zgłaszane; dowiązania symboliczne i punkty połączeń Windows nie są śledzone.

## Współtworzenie

Instrukcje kompilacji i podręcznik użytkownika są po angielsku. Tłumaczenia utrzymujemy w Git; CI wymaga kompletnych katalogów i plików README sprawdzonych względem aktualnych źródeł angielskich.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
