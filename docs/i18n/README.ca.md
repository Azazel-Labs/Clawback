# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · Català · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback mostra on va a parar l'espai del disc. Escaneja una unitat o carpeta i representa cada fitxer amb un rectangle proporcional a l'espai que ocupa, dins de la seva carpeta. Funciona a Windows, macOS i Linux, amb una aplicació d'escriptori i un mode de terminal.

![Clawback](../images/desktop.png)

*Dades de demostració: fitxers i mides de ficció*

## Funcions

Explora l'arbre ordenat per mida i el desglossament per tipus de fitxer. Fes doble clic per ampliar i torna amb Enrere o Escape. El mapa es pot utilitzar durant l'escaneig, amb pausa, represa i actualitzacions en directe. Obre fitxers, consulta propietats o envia'ls a la paperera; es pot desactivar l'eliminació. Deu paletes, densitat i càlcul de mides configurables, i interfície en 31 idiomes.

![Clawback](../images/explore.png)

## Instal·lació

Baixa la versió més recent. A Windows, executa `clawback.exe`; a macOS, arrossega `Clawback.app` a Applications; a Linux, executa `clawback`. Tria **Fitxer → Obre una carpeta**. La instal·lació des de crates.io requereix Rust 1.95 o posterior i utilitza les fonts xineses, japoneses i coreanes del sistema; les versions descarregables les inclouen.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Les ordres següents obren l’aplicació d’escriptori, el mapa interactiu al terminal i un informe de text. El terminal permet navegar i veure actualitzacions en directe, però no fer accions sobre fitxers. Els informes també funcionen en scripts.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Escaneig

A Windows, Turbo llegeix més ràpidament un volum NTFS local sencer mitjançant un procés auxiliar de només lectura que demana autorització UAC. L'escaneig normal continua disponible. macOS llegeix metadades per lots sense permisos d'administrador. L'escàner de directoris ajusta els fils automàticament. S'indiquen les carpetes il·legibles i no se segueixen enllaços simbòlics ni unions de Windows.

## Contribucions

Les instruccions de compilació i la guia d'ús són en anglès. Les traduccions es mantenen a Git; CI exigeix catàlegs complets i README revisats amb les fonts angleses actuals.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
