# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · Afrikaans · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback wys waar jou skyfspasie heen is. Dit skandeer 'n skyf of vouer en wys lêers as reghoeke waarvan die grootte hul ruimtegebruik voorstel, binne hul vouers. Beskikbaar vir Windows, macOS en Linux, met 'n lessenaar- en terminaalkoppelvlak.

![Clawback](../images/desktop.png)

*Demo-data - fiktiewe lêers en groottes*

## Kenmerke

Verken die boom volgens grootte en lêertipe; dubbelklik om in te zoem en gebruik Terug of Escape om terug te gaan. Gebruik die kaart terwyl dit skandeer, onderbreek en hervat, en sien lewendige opdaterings. Maak lêers oop, bekyk eienskappe of skuif dit na die asblik. Verwydering kan afgeskakel word. Tien kleurpalette, aanpasbare digtheid en grootteberekening, en 31 koppelvlaktale.

![Clawback](../images/explore.png)

## Installeer

Laai die jongste weergawe af. Op Windows, voer `clawback.exe` uit; op macOS, sleep `Clawback.app` na Applications; op Linux, voer `clawback` uit. Kies dan **Lêer → Maak vouer oop**. Installasie vanaf crates.io vereis Rust 1.95 of nuwer en gebruik die stelsel se Chinese, Japannese en Koreaanse lettertipes; aflaaibare vrystellings sluit dit in.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminaal

Die opdragte hieronder begin die lessenaartoepassing, die interaktiewe terminaalkaart en 'n teksverslag. Die terminaal ondersteun navigasie en lewendige opdaterings, maar geen lêeraksies nie. Verslae werk ook in skrifte.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Skandering

Op Windows kan Turbo 'n hele plaaslike NTFS-volume vinniger lees met 'n aparte, leesalleen-hulpproses wat UAC-goedkeuring vereis. Gewone skandering bly beskikbaar. macOS lees metadata in groepe sonder administrateurregte. Die gidskandeerder pas sy draadtelling outomaties aan. Onleesbare vouers word gerapporteer; simboliese skakels en Windows-aansluitings word nie gevolg nie.

## Bydra

Bou-instruksies en die gebruikersgids is in Engels. Vertalings word in Git onderhou; CI vereis volledige katalogusse en README-lêers wat teen die huidige Engelse bronne nagegaan is.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
