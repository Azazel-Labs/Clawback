# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · Svenska · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback visar vad som tar upp diskutrymmet. Det skannar en enhet eller mapp och visar filer som rektanglar efter utrymmesanvändning, inuti sina mappar. För Windows, macOS och Linux, med skrivbordsapp och terminalläge.

![Clawback](../images/desktop.png)

*Skärmbilderna visar en inbyggd demoenhet; filnamnen och storlekarna är påhittade.*

## Funktioner

Utforska trädet sorterat efter storlek och fördelningen per filtyp. Dubbelklicka för att zooma in och använd Tillbaka eller Escape för att återvända. Kartan fungerar under skanning, med paus, återupptagning och uppdateringar i realtid. Öppna filer, visa egenskaper eller flytta till papperskorgen; radering kan stängas av. Tio färgpaletter, justerbar täthet och storleksberäkning samt gränssnitt på 31 språk.

![Clawback](../images/explore.png)

## Installation

Hämta den senaste utgåvan. Kör `clawback.exe` på Windows, dra `Clawback.app` till Applications på macOS eller kör `clawback` på Linux. Välj **Fil → Öppna mapp**. Installation från crates.io kräver Rust 1.95 eller senare och använder systemets kinesiska, japanska och koreanska typsnitt; färdiga utgåvor innehåller egna.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Kommandona nedan startar skrivbordsappen, den interaktiva terminalkartan och en textrapport. Terminalen stöder navigering och uppdateringar i realtid, men inga filåtgärder. Rapporter fungerar också i skript.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Skanning

På Windows läser Turbo en hel lokal NTFS-volym snabbare via en separat hjälpprocess som bara läser och begär UAC-godkännande. Vanlig skanning finns fortfarande tillgänglig. macOS läser metadata i grupper utan administratörsrättigheter. Mappskannern anpassar antalet trådar automatiskt. Oläsbara mappar rapporteras; symboliska länkar och Windows-kopplingspunkter följs inte.

## Bidra

Bygginstruktionerna och användarhandboken är på engelska. Översättningar underhålls i Git; CI kräver fullständiga kataloger och README-filer granskade mot de aktuella engelska källorna.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
