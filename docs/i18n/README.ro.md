# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · Română · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback arată ce ocupă spațiul de pe disc. Scanează o unitate sau un dosar și afișează fișierele ca dreptunghiuri proporționale cu spațiul folosit, în interiorul dosarelor lor. Funcționează pe Windows, macOS și Linux, cu aplicație grafică și mod terminal.

![Clawback](../images/desktop.png)

*Capturile de ecran folosesc o unitate demonstrativă integrată; numele și dimensiunile fișierelor sunt fictive.*

## Funcții

Explorează arborele sortat după dimensiune și distribuția pe tipuri de fișiere. Dublu clic mărește, iar Înapoi sau Escape revine. Harta funcționează în timpul scanării, cu pauză, reluare și actualizări în timp real. Deschide fișiere, vezi proprietăți sau mută-le în coș; ștergerea poate fi dezactivată. Zece palete, densitate și calcul al dimensiunilor configurabile, interfață în 31 de limbi.

![Clawback](../images/explore.png)

## Instalare

Descarcă cea mai recentă versiune. Pe Windows rulează `clawback.exe`, pe macOS trage `Clawback.app` în Applications, iar pe Linux rulează `clawback`. Alege **Fișier → Deschide un dosar**. Instalarea din crates.io necesită Rust 1.95 sau mai nou și folosește fonturile chinezești, japoneze și coreene ale sistemului; versiunile descărcabile le includ.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Comenzile de mai jos deschid aplicația grafică, harta interactivă din terminal și un raport text. Terminalul permite navigarea și actualizările în timp real, dar nu și operațiile asupra fișierelor. Rapoartele funcționează și în scripturi.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Scanare

Pe Windows, Turbo citește mai rapid un întreg volum NTFS local printr-un proces auxiliar separat, doar pentru citire, care cere aprobarea UAC. Scanarea normală rămâne disponibilă. macOS citește metadatele în loturi fără drepturi de administrator. Scanerul de directoare ajustează automat numărul de fire. Dosarele care nu pot fi citite sunt raportate; legăturile simbolice și joncțiunile Windows nu sunt urmate.

## Contribuții

Instrucțiunile de compilare și ghidul utilizatorului sunt în engleză. Traducerile sunt întreținute în Git; CI cere cataloage complete și fișiere README verificate față de sursele englezești actuale.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
