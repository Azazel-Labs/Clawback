# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · Dansk · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback viser, hvad der optager diskpladsen. Det scanner et drev eller en mappe og viser hver fil som et rektangel, der svarer til pladsforbruget, indlejret i mapperne. Til Windows, macOS og Linux med både skrivebordsapp og terminaltilstand.

![Clawback](../images/desktop.png)

*Demodata - fiktive filer og størrelser*

## Funktioner

Udforsk træet sorteret efter størrelse og oversigten over filtyper. Dobbeltklik for at zoome ind, og brug Tilbage eller Escape for at gå tilbage. Kortet kan bruges under scanning med pause, genoptagelse og løbende opdateringer. Åbn filer, vis egenskaber eller flyt dem til papirkurven; sletning kan slås fra. Ti farvepaletter, justerbar tæthed og størrelsesberegning samt 31 sprog.

![Clawback](../images/explore.png)

## Installation

Hent den nyeste udgivelse. Kør `clawback.exe` på Windows, træk `Clawback.app` til Applications på macOS, eller kør `clawback` på Linux. Vælg **Fil → Åbn mappe**. Installation fra crates.io kræver Rust 1.95 eller nyere og bruger systemets kinesiske, japanske og koreanske skrifttyper; de færdige udgivelser indeholder dem.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Kommandoerne nedenfor starter skrivebordsappen, det interaktive terminalkort og en tekstrapport. Terminalen understøtter navigation og løbende opdateringer, men ingen filhandlinger. Rapporter kan bruges i scripts.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Scanning

På Windows kan Turbo læse en hel lokal NTFS-diskenhed hurtigere via en separat skrivebeskyttet hjælpeproces, der kræver UAC-godkendelse. Almindelig scanning er stadig tilgængelig. macOS læser metadata i grupper uden administratorrettigheder. Mappescanneren tilpasser automatisk antallet af tråde. Mapper, der ikke kan læses, vises; symbolske links og Windows-junctions følges ikke.

## Bidrag

Byggevejledningen og brugervejledningen er på engelsk. Oversættelser vedligeholdes i Git; CI kræver komplette kataloger og README-filer gennemgået mod de aktuelle engelske kilder.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
