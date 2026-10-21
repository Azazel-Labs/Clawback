# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · Deutsch · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback zeigt, was deinen Speicherplatz belegt. Es scannt ein Laufwerk oder einen Ordner und stellt Dateien als Rechtecke dar, deren Fläche dem Platzbedarf entspricht, verschachtelt in ihren Ordnern. Für Windows, macOS und Linux, als Desktop-App und im Terminal.

![Clawback](../images/desktop.png)

*Demodaten – erfundene Dateien und Größen*

## Funktionen

Erkunde den nach Größe sortierten Verzeichnisbaum und die Übersicht nach Dateityp. Doppelklick zoomt hinein, Zurück oder Escape führt zurück. Die Karte ist schon während des Scans nutzbar, mit Pause, Fortsetzen und Live-Aktualisierung. Öffne Dateien, zeige Eigenschaften oder verschiebe sie in den Papierkorb; Löschen lässt sich deaktivieren. Zehn Farbpaletten, einstellbare Dichte und Größenberechnung sowie 31 Oberflächensprachen.

![Clawback](../images/explore.png)

## Installation

Lade die neueste Veröffentlichung herunter. Starte unter Windows `clawback.exe`, ziehe unter macOS `Clawback.app` nach Applications und starte unter Linux `clawback`. Wähle **Datei → Ordner öffnen**. Die Installation über crates.io benötigt Rust 1.95 oder neuer und verwendet chinesische, japanische und koreanische Systemschriften; die fertigen Downloads enthalten eigene.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Die folgenden Befehle starten die Desktop-App, die interaktive Terminalkarte und einen Textbericht. Das Terminal unterstützt Navigation und Live-Aktualisierungen, aber keine Dateiaktionen. Berichte lassen sich in Skripten verwenden.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Scannen

Unter Windows liest Turbo ein ganzes lokales NTFS-Volume schneller über einen separaten, nur lesenden Hilfsprozess, der eine UAC-Zustimmung benötigt. Normale Scans bleiben verfügbar. macOS liest Metadaten stapelweise ohne Administratorrechte. Der Verzeichnisscanner passt die Threadanzahl automatisch an. Nicht lesbare Ordner werden aufgeführt; symbolischen Links und Windows-Verzeichnisverbindungen wird nicht gefolgt.

## Mitwirken

Bauanleitung und Benutzerhandbuch sind auf Englisch. Übersetzungen werden in Git gepflegt; CI verlangt vollständige Kataloge und READMEs, die anhand der aktuellen englischen Quellen geprüft wurden.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
