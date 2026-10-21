# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · Français · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback montre ce qui occupe votre disque. Il analyse un lecteur ou un dossier et représente chaque fichier par un rectangle proportionnel à son espace occupé, imbriqué dans ses dossiers. Disponible sous Windows, macOS et Linux, avec une application de bureau et un mode terminal.

![Clawback](../images/desktop.png)

*Données de démonstration : fichiers et tailles fictifs*

## Fonctionnalités

Explorez l'arborescence triée par taille et la répartition par type de fichier. Double-cliquez pour zoomer, puis utilisez Retour ou Échap. La carte fonctionne pendant l'analyse, avec pause, reprise et mises à jour en direct. Ouvrez les fichiers, consultez leurs propriétés ou envoyez-les à la corbeille ; la suppression peut être désactivée. Dix palettes, densité et calcul des tailles réglables, interface en 31 langues.

![Clawback](../images/explore.png)

## Installation

Téléchargez la dernière version. Sous Windows, lancez `clawback.exe` ; sous macOS, glissez `Clawback.app` dans Applications ; sous Linux, lancez `clawback`. Choisissez **Fichier → Ouvrir un dossier**. L'installation depuis crates.io nécessite Rust 1.95 ou ultérieur et utilise les polices chinoises, japonaises et coréennes du système ; les versions téléchargeables les incluent.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Les commandes suivantes ouvrent l'application de bureau, la carte interactive du terminal et un rapport texte. Le terminal permet la navigation et les mises à jour en direct, mais aucune action sur les fichiers. Les rapports fonctionnent aussi dans les scripts.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Analyse

Sous Windows, Turbo lit plus rapidement un volume NTFS local entier grâce à un processus auxiliaire distinct en lecture seule, qui demande une autorisation UAC. L'analyse normale reste disponible. macOS lit les métadonnées par lots sans droits administrateur. L'analyse des répertoires adapte automatiquement ses fils d'exécution. Les dossiers illisibles sont signalés ; les liens symboliques et jonctions Windows ne sont pas suivis.

## Contribuer

Les instructions de compilation et le guide d'utilisation sont en anglais. Les traductions sont gérées dans Git ; la CI exige des catalogues complets et des README vérifiés par rapport aux sources anglaises actuelles.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
