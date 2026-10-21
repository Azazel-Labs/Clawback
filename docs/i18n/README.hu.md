# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · Magyar · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

A Clawback megmutatja, mi foglalja a lemezterületet. Átvizsgál egy meghajtót vagy mappát, és a fájlokat a helyfoglalásukkal arányos téglalapokként jeleníti meg a mappákon belül. Windows, macOS és Linux rendszeren fut, grafikus és terminálos felülettel.

![Clawback](../images/desktop.png)

*Demo adatok – kitalált fájlok és méretek*

## Funkciók

Böngésszen a méret szerint rendezett fában és a fájltípusok szerinti bontásban. Dupla kattintással nagyíthat, a Vissza vagy Escape gombbal visszatérhet. A térkép vizsgálat közben is használható, szüneteltetéssel, folytatással és élő frissítéssel. Fájlokat nyithat meg, megtekintheti tulajdonságaikat vagy a kukába helyezheti őket; a törlés letiltható. Tíz színpaletta, állítható sűrűség és méretszámítás, valamint 31 felületi nyelv.

![Clawback](../images/explore.png)

## Telepítés

Töltse le a legújabb kiadást. Windowson futtassa a `clawback.exe` fájlt, macOS-en húzza a `Clawback.app` alkalmazást az Applications mappába, Linuxon futtassa a `clawback` parancsot. Válassza a **Fájl → Mappa megnyitása** menüt. A crates.io-ról történő telepítéshez Rust 1.95 vagy újabb szükséges; ez a rendszer kínai, japán és koreai betűkészleteit használja, a letölthető kiadások sajátokat tartalmaznak.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminál

Az alábbi parancsok a grafikus alkalmazást, az interaktív termináltérképet és a szöveges jelentést indítják. A terminál támogatja a böngészést és az élő frissítést, de a fájlműveleteket nem. A jelentések parancsfájlokból is használhatók.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Vizsgálat

Windowson a Turbo külön, csak olvasó segédfolyamattal gyorsabban olvassa be a teljes helyi NTFS-kötetet; ehhez UAC-jóváhagyást kér. A normál vizsgálat továbbra is elérhető. A macOS csoportosan olvassa a metaadatokat rendszergazdai jogok nélkül. A mappavizsgáló automatikusan állítja a szálak számát. Jelzi az olvashatatlan mappákat; nem követ szimbolikus hivatkozásokat vagy Windows-csatolási pontokat.

## Közreműködés

A forráskód lefordításának útmutatója és a felhasználói kézikönyv angol nyelvű. A nyelvi fordításokat Gitben kezeljük; a CI teljes katalógusokat és az aktuális angol forrásokkal összevetett README-ket követel meg.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
