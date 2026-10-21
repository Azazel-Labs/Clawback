# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · Српски · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback показује шта заузима простор на диску. Скенира диск или фасциклу и приказује датотеке као правоугаонике сразмерне заузетом простору, унутар њихових фасцикли. Ради на Windows-у, macOS-у и Linux-у, са графичком апликацијом и терминалским режимом.

![Clawback](../images/desktop.png)

*Снимци екрана приказују уграђени демонстрациони диск; имена и величине датотека су измишљени.*

## Могућности

Прегледајте стабло по величини и расподелу по типовима датотека. Двоструки клик увећава, а Назад или Escape враћа. Мапа ради током скенирања, уз паузу, наставак и ажурирања уживо. Отворите датотеке, погледајте својства или их преместите у корпу; брисање се може искључити. Десет палета, подесива густина и рачунање величина, интерфејс на 31 језику.

![Clawback](../images/explore.png)

## Инсталација

Преузмите најновије издање. На Windows-у покрените `clawback.exe`, на macOS-у превуците `Clawback.app` у Applications, а на Linux-у покрените `clawback`. Изаберите **Датотека → Отвори фасциклу**. Инсталација са crates.io захтева Rust 1.95 или новији и користи системске кинеске, јапанске и корејске фонтове; готова издања их садрже.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Терминал

Следеће команде покрећу графичку апликацију, интерактивну терминалску мапу и текстуални извештај. Терминал подржава навигацију и ажурирања уживо, али не и радње над датотекама. Извештаји раде и у скриптама.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Скенирање

На Windows-у Turbo брже чита цео локални NTFS волумен преко засебног помоћног процеса само за читање који тражи UAC одобрење. Обично скенирање остаје доступно. macOS чита метаподатке у групама без администраторских права. Скенер директоријума аутоматски прилагођава број нити. Нечитљиве фасцикле се пријављују; симболичке везе и Windows спојне тачке се не прате.

## Доприноси

Упутства за изградњу и кориснички приручник су на енглеском. Преводи се одржавају у Git-у; CI захтева потпуне каталоге и README датотеке проверене према актуелним енглеским изворима.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
