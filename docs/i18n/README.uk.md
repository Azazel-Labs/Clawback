# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · Українська · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback показує, що займає місце на диску. Він сканує диск або теку та відображає файли прямокутниками, площа яких відповідає зайнятому місцю, усередині їхніх тек. Працює у Windows, macOS і Linux, із графічним інтерфейсом та режимом термінала.

![Clawback](../images/desktop.png)

*На знімках екрана показано вбудований демонстраційний диск; назви та розміри файлів вигадані.*

## Можливості

Переглядайте дерево за розміром і розподіл за типами файлів. Подвійне клацання наближає, Назад або Escape повертає. Карта працює під час сканування, з паузою, продовженням і оновленнями наживо. Відкривайте файли, переглядайте властивості або переміщуйте їх до кошика; видалення можна вимкнути. Десять палітр, налаштування щільності й обчислення розмірів, інтерфейс 31 мовою.

![Clawback](../images/explore.png)

## Встановлення

Завантажте найновіший випуск. У Windows запустіть `clawback.exe`, у macOS перетягніть `Clawback.app` до Applications, у Linux запустіть `clawback`. Виберіть **Файл → Відкрити теку**. Встановлення з crates.io потребує Rust 1.95 або новішого та використовує системні китайські, японські й корейські шрифти; готові випуски містять власні.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Термінал

Наведені нижче команди запускають графічну програму, інтерактивну карту в терміналі та текстовий звіт. Термінал підтримує навігацію й оновлення наживо, але не дії з файлами. Звіти працюють також у скриптах.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Сканування

У Windows Turbo швидше читає весь локальний том NTFS через окремий допоміжний процес лише для читання, який запитує згоду UAC. Звичайне сканування залишається доступним. macOS читає метадані пакетами без прав адміністратора. Сканер каталогів автоматично регулює кількість потоків. Недоступні теки перелічуються; символічні посилання й точки з'єднання Windows не відстежуються.

## Участь

Інструкції зі збирання та посібник користувача доступні англійською. Переклади зберігаються в Git; CI вимагає повних каталогів і README, перевірених за поточними англійськими джерелами.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
