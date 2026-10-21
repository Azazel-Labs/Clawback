# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · Suomi · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback näyttää, mihin levytila kuluu. Se tutkii aseman tai kansion ja esittää tiedostot niiden tilankäyttöä vastaavina suorakulmioina kansioidensa sisällä. Toimii Windowsissa, macOS:ssä ja Linuxissa työpöytäsovelluksena ja päätteessä.

![Clawback](../images/desktop.png)

*Demodata - kuvitteelliset tiedostot ja koot*

## Ominaisuudet

Selaa koon mukaan järjestettyä puuta ja tiedostotyyppien yhteenvetoa. Kaksoisnapsautus lähentää, Takaisin tai Escape palauttaa. Karttaa voi käyttää jo skannauksen aikana; mukana ovat tauotus, jatkaminen ja reaaliaikaiset päivitykset. Avaa tiedostoja, katso ominaisuuksia tai siirrä roskakoriin; poistamisen voi estää. Kymmenen väripalettia, säädettävä tiheys ja kokojen laskenta sekä 31 käyttöliittymäkieltä.

![Clawback](../images/explore.png)

## Asennus

Lataa uusin julkaisu. Käynnistä Windowsissa `clawback.exe`, vedä macOS:ssä `Clawback.app` Applications-kansioon tai suorita Linuxissa `clawback`. Valitse **Tiedosto → Avaa kansio**. Asennus crates.io-palvelusta vaatii Rust 1.95:n tai uudemman ja käyttää järjestelmän kiinalaisia, japanilaisia ja korealaisia fontteja; valmiit julkaisut sisältävät ne.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Pääte

Seuraavat komennot avaavat työpöytäsovelluksen, vuorovaikutteisen päätekartan ja tekstiraportin. Pääte tukee selausta ja reaaliaikaisia päivityksiä, mutta ei tiedostotoimintoja. Raportteja voi käyttää skripteissä.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Skannaus

Windowsissa Turbo lukee kokonaisen paikallisen NTFS-taltion nopeammin erillisellä, vain lukevalla apuprosessilla, joka pyytää UAC-hyväksynnän. Tavallinen skannaus on edelleen käytettävissä. macOS lukee metatietoja erissä ilman ylläpitäjän oikeuksia. Hakemistoskanneri säätää säikeiden määrää automaattisesti. Lukukelvottomat kansiot ilmoitetaan; symbolisia linkkejä tai Windowsin liitoskohtia ei seurata.

## Osallistuminen

Koontiohje ja käyttöopas ovat englanniksi. Käännöksiä ylläpidetään Gitissä; CI edellyttää täydellisiä luetteloita ja README-tiedostoja, jotka on tarkistettu nykyisiä englanninkielisiä lähteitä vasten.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
