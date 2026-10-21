# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · 日本語 · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback は、ディスク容量を何が使っているかを可視化します。ドライブやフォルダーをスキャンし、使用量に比例する長方形でファイルを表示します。長方形はフォルダーの階層に沿って入れ子になります。Windows、macOS、Linux に対応し、デスクトップとターミナルの両方で使えます。

![Clawback](../images/desktop.png)

*デモ データ - 架空のファイルとサイズ*

## 機能

サイズ順のツリーとファイル形式別の内訳を確認できます。ダブルクリックで拡大し、戻るボタンや Escape で戻れます。スキャン中もマップを操作でき、一時停止、再開、変更の自動反映に対応しています。ファイルを開く、プロパティを表示する、ごみ箱へ移す操作ができ、削除機能は無効化できます。10 種類の配色、密度やサイズ計算の設定、31 言語のインターフェースを備えています。

![Clawback](../images/explore.png)

## インストール

最新リリースをダウンロードしてください。Windows では `clawback.exe` を実行し、macOS では `Clawback.app` を Applications にドラッグし、Linux では `clawback` を実行します。続いて **ファイル → フォルダーを開く** を選びます。crates.io からのインストールには Rust 1.95 以降が必要です。このビルドはシステムの中国語・日本語・韓国語フォントを使用し、配布版にはフォントが同梱されています。

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## ターミナル

以下のコマンドで、デスクトップ、対話式ターミナルマップ、テキストレポートをそれぞれ起動します。ターミナルは移動と自動更新に対応しますが、ファイル操作には対応していません。レポートはスクリプトでも使えます。

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## スキャン

Windows の Turbo は、UAC の承認を求める独立した読み取り専用ヘルパーを使い、ローカル NTFS ボリューム全体を高速に読み取ります。通常のスキャンも利用できます。macOS は管理者権限なしでメタデータを一括取得します。ディレクトリスキャナーはスレッド数を自動調整します。読み取れないフォルダーは報告され、シンボリックリンクや Windows のジャンクションはたどりません。

## 貢献

ビルド手順とユーザーガイドは英語です。翻訳は Git で管理され、CI はすべての項目を含むカタログと、現在の英語ソースに照らして確認された README を必須とします。

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
