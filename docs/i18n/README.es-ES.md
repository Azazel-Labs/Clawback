# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · Español (España) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback muestra qué ocupa el espacio de tu disco. Analiza una unidad o carpeta y representa los archivos con rectángulos proporcionales al espacio que ocupan, dentro de sus carpetas. Funciona en Windows, macOS y Linux, con aplicación de escritorio y modo de terminal.

![Clawback](../images/desktop.png)

*Datos de demostración: archivos y tamaños ficticios*

## Funciones

Explora el árbol ordenado por tamaño y el desglose por tipo de archivo. Haz doble clic para ampliar y usa Atrás o Escape para volver. El mapa funciona durante el análisis, con pausa, reanudación y actualizaciones en directo. Abre archivos, consulta sus propiedades o muévelos a la papelera; puedes desactivar la eliminación. Diez paletas, densidad y cálculo de tamaños ajustables, e interfaz en 31 idiomas.

![Clawback](../images/explore.png)

## Instalación

Descarga la última versión. En Windows ejecuta `clawback.exe`; en macOS arrastra `Clawback.app` a Applications; en Linux ejecuta `clawback`. Selecciona **Archivo → Abrir carpeta**. La instalación desde crates.io requiere Rust 1.95 o posterior y utiliza las fuentes chinas, japonesas y coreanas del sistema; las versiones descargables las incluyen.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Los siguientes comandos abren la aplicación de escritorio, el mapa interactivo de terminal y un informe de texto. El terminal admite navegación y actualizaciones en directo, pero no operaciones sobre archivos. Los informes también funcionan en scripts.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Análisis

En Windows, Turbo lee más deprisa un volumen NTFS local completo mediante un proceso auxiliar independiente de solo lectura que solicita autorización UAC. El análisis normal sigue disponible. macOS lee metadatos por lotes sin permisos de administrador. El analizador de directorios ajusta sus hilos automáticamente. Se indican las carpetas que no se pueden leer; no se siguen enlaces simbólicos ni uniones de Windows.

## Colaboración

Las instrucciones de compilación y la guía de uso están en inglés. Las traducciones se mantienen en Git; CI exige catálogos completos y README revisados con las fuentes inglesas actuales.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
