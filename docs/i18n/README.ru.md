# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · Русский · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback показывает, чем занято место на диске. Он сканирует диск или папку и отображает файлы прямоугольниками, площадь которых соответствует занимаемому месту, внутри их папок. Работает в Windows, macOS и Linux, с графическим интерфейсом и режимом терминала.

![Clawback](../images/desktop.png)

*На снимках экрана показан встроенный демонстрационный диск; имена и размеры файлов вымышлены.*

## Возможности

Просматривайте дерево по размеру и сводку по типам файлов. Двойной щелчок приближает, Назад или Escape возвращает. Карта доступна во время сканирования; есть пауза, продолжение и обновления в реальном времени. Открывайте файлы, смотрите свойства или перемещайте их в корзину; удаление можно отключить. Десять палитр, настройка плотности и подсчёта размеров, интерфейс на 31 языке.

![Clawback](../images/explore.png)

## Установка

Скачайте последний выпуск. В Windows запустите `clawback.exe`, в macOS перетащите `Clawback.app` в Applications, в Linux запустите `clawback`. Выберите **Файл → Открыть папку**. Установка из crates.io требует Rust 1.95 или новее и использует системные китайские, японские и корейские шрифты; готовые сборки содержат собственные.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Терминал

Команды ниже запускают графическое приложение, интерактивную карту в терминале и текстовый отчёт. Терминал поддерживает навигацию и обновления в реальном времени, но не действия с файлами. Отчёты работают и в скриптах.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Сканирование

В Windows Turbo быстрее читает весь локальный том NTFS через отдельный вспомогательный процесс только для чтения, запрашивающий согласие UAC. Обычное сканирование остаётся доступным. macOS читает метаданные пакетами без прав администратора. Сканер каталогов автоматически регулирует число потоков. Недоступные папки перечисляются; символические ссылки и точки соединения Windows не прослеживаются.

## Участие

Инструкции по сборке и руководство пользователя доступны на английском. Переводы хранятся в Git; CI требует полных каталогов и README, проверенных по текущим английским исходникам.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
