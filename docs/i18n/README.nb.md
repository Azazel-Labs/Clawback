# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · Norsk bokmål · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback viser hva som bruker diskplassen din. Det skanner en stasjon eller mappe og viser filer som rektangler med størrelse etter plassbruk, inne i mappene sine. For Windows, macOS og Linux, med skrivebordsapp og terminalmodus.

![Clawback](../images/desktop.png)

*Demodata - fiktive filer og størrelser*

## Funksjoner

Utforsk treet sortert etter størrelse og oversikten over filtyper. Dobbeltklikk for å zoome inn, og bruk Tilbake eller Escape for å gå tilbake. Kartet virker under skanning, med pause, fortsettelse og løpende oppdateringer. Åpne filer, vis egenskaper eller flytt dem til papirkurven; sletting kan slås av. Ti fargepaletter, justerbar tetthet og størrelsesberegning, og grensesnitt på 31 språk.

![Clawback](../images/explore.png)

## Installasjon

Last ned nyeste utgivelse. Kjør `clawback.exe` på Windows, dra `Clawback.app` til Applications på macOS, eller kjør `clawback` på Linux. Velg **Fil → Åpne mappen**. Installasjon fra crates.io krever Rust 1.95 eller nyere og bruker systemets kinesiske, japanske og koreanske skrifter; ferdige utgivelser inkluderer dem.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Kommandoene nedenfor starter skrivebordsappen, det interaktive terminalkartet og en tekstrapport. Terminalen støtter navigasjon og løpende oppdateringer, men ikke filhandlinger. Rapporter fungerer også i skript.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Skanning

På Windows leser Turbo et helt lokalt NTFS-volum raskere gjennom en separat hjelpeprosess som bare leser, og som ber om UAC-godkjenning. Vanlig skanning er fortsatt tilgjengelig. macOS leser metadata i grupper uten administratorrettigheter. Mappeskanneren tilpasser trådantallet automatisk. Mapper som ikke kan leses, rapporteres; symbolske lenker og Windows-koblingspunkter følges ikke.

## Bidra

Byggeinstruksjonene og brukerveiledningen er på engelsk. Oversettelser vedlikeholdes i Git; CI krever komplette kataloger og README-filer gjennomgått mot de gjeldende engelske kildene.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
