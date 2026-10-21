# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · Nederlands · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback laat zien wat je schijfruimte gebruikt. Het scant een schijf of map en toont bestanden als rechthoeken naar ruimtegebruik, genest in hun mappen. Voor Windows, macOS en Linux, met een desktopapp en terminalmodus.

![Clawback](../images/desktop.png)

*Demogegevens - fictieve bestanden en groottes*

## Functies

Verken de boom op grootte en het overzicht per bestandstype. Dubbelklik om in te zoomen en gebruik Terug of Escape om terug te gaan. De kaart werkt tijdens het scannen, met pauzeren, hervatten en live bijwerken. Open bestanden, bekijk eigenschappen of verplaats ze naar de prullenbak; verwijderen kan worden uitgeschakeld. Tien kleurenpaletten, instelbare dichtheid en grootteberekening, en een interface in 31 talen.

![Clawback](../images/explore.png)

## Installatie

Download de nieuwste versie. Start op Windows `clawback.exe`, sleep op macOS `Clawback.app` naar Applications en start op Linux `clawback`. Kies **Bestand → Map openen**. Installatie via crates.io vereist Rust 1.95 of nieuwer en gebruikt de Chinese, Japanse en Koreaanse systeemlettertypen; de kant-en-klare downloads bevatten eigen lettertypen.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

De volgende opdrachten starten de desktopapp, de interactieve terminalkaart en een tekstrapport. De terminal ondersteunt navigatie en live updates, maar geen bestandsacties. Rapporten werken ook in scripts.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Scannen

Op Windows leest Turbo een volledig lokaal NTFS-volume sneller via een afzonderlijk hulpproces dat alleen leest en UAC-toestemming vraagt. Normaal scannen blijft beschikbaar. macOS leest metadata in groepen zonder beheerdersrechten. De mappenscanner past het aantal threads automatisch aan. Onleesbare mappen worden gemeld; symbolische koppelingen en Windows-junctions worden niet gevolgd.

## Bijdragen

De bouwinstructies en gebruikershandleiding zijn in het Engels. Vertalingen worden beheerd in Git; CI vereist volledige catalogi en README-bestanden die tegen de huidige Engelse bronnen zijn gecontroleerd.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
