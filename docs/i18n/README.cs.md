# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · Čeština · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback ukazuje, co zabírá místo na disku. Prohledá disk nebo složku a zobrazí soubory jako obdélníky s plochou úměrnou jejich velikosti, vnořené do složek. Funguje ve Windows, macOS a Linuxu jako grafická aplikace i v terminálu.

![Clawback](../images/desktop.png)

*Demo data - fiktivní soubory a velikosti*

## Funkce

Procházejte strom seřazený podle velikosti a přehled typů souborů. Dvojklik přiblíží složku, Zpět nebo Escape vás vrátí. Mapa funguje už během skenování; podporuje pozastavení, pokračování a živé aktualizace. Soubory lze otevřít, zobrazit jejich vlastnosti nebo přesunout do koše; mazání lze zakázat. Deset barevných palet, nastavitelná hustota a výpočet velikostí, rozhraní ve 31 jazycích.

![Clawback](../images/explore.png)

## Instalace

Stáhněte nejnovější vydání. Ve Windows spusťte `clawback.exe`, v macOS přetáhněte `Clawback.app` do Applications a v Linuxu spusťte `clawback`. Vyberte **Soubor → Otevřít složku**. Instalace z crates.io vyžaduje Rust 1.95 nebo novější a používá systémová čínská, japonská a korejská písma; vydané balíčky je obsahují.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminál

Následující příkazy spustí grafickou aplikaci, interaktivní mapu v terminálu a textový přehled. Terminál podporuje navigaci a živé aktualizace, nikoli operace se soubory. Přehledy lze použít ve skriptech.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Skenování

Ve Windows umí Turbo rychleji načíst celý místní svazek NTFS pomocí samostatného pomocného procesu pouze pro čtení, který vyžaduje souhlas UAC. Běžné skenování zůstává dostupné. macOS čte metadata dávkově bez práv správce. Procházení adresářů automaticky přizpůsobuje počet vláken. Nečitelné složky jsou uvedeny v přehledu; symbolické odkazy ani spojení Windows se nenásledují.

## Přispívání

Návod k sestavení a uživatelská příručka jsou v angličtině. Překlady se spravují v Gitu; CI vyžaduje úplné katalogy a README zkontrolované vůči aktuálním anglickým zdrojům.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
