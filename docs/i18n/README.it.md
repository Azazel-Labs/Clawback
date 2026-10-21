# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · Italiano · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback mostra cosa occupa lo spazio sul disco. Analizza un'unità o una cartella e rappresenta i file con rettangoli proporzionali allo spazio utilizzato, all'interno delle cartelle. Funziona su Windows, macOS e Linux, con un'app desktop e una modalità terminale.

![Clawback](../images/desktop.png)

*Dati demo: file e dimensioni fittizi*

## Funzionalità

Esplora l'albero ordinato per dimensione e il riepilogo per tipo di file. Fai doppio clic per ingrandire e usa Indietro o Escape per tornare. La mappa funziona durante la scansione, con pausa, ripresa e aggiornamenti in tempo reale. Apri file, visualizza proprietà o spostali nel cestino; l'eliminazione può essere disattivata. Dieci tavolozze, densità e calcolo delle dimensioni regolabili, interfaccia in 31 lingue.

![Clawback](../images/explore.png)

## Installazione

Scarica l'ultima versione. Su Windows avvia `clawback.exe`, su macOS trascina `Clawback.app` in Applications e su Linux avvia `clawback`. Seleziona **File → Apri cartella**. L'installazione da crates.io richiede Rust 1.95 o successivo e usa i caratteri cinesi, giapponesi e coreani del sistema; le versioni scaricabili li includono.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminale

I comandi seguenti avviano l'app desktop, la mappa interattiva nel terminale e un rapporto testuale. Il terminale supporta navigazione e aggiornamenti in tempo reale, ma non operazioni sui file. I rapporti funzionano anche negli script.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Scansione

Su Windows, Turbo legge più rapidamente un intero volume NTFS locale tramite un processo ausiliario separato di sola lettura che richiede l'autorizzazione UAC. La scansione normale resta disponibile. macOS legge i metadati in gruppi senza privilegi di amministratore. Lo scanner delle directory adatta automaticamente i thread. Le cartelle illeggibili vengono segnalate; i collegamenti simbolici e le giunzioni Windows non vengono seguiti.

## Contribuire

Le istruzioni di compilazione e la guida utente sono in inglese. Le traduzioni sono gestite in Git; la CI richiede cataloghi completi e README verificati rispetto alle fonti inglesi attuali.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
