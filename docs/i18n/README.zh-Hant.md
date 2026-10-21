# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · 繁體中文

Clawback 幫你了解磁碟空間被什麼占用。它掃描磁碟機或資料夾，將檔案顯示為面積與占用空間成比例的矩形，並依資料夾階層巢狀排列。支援 Windows、macOS 和 Linux，提供桌面應用程式與終端機模式。

![Clawback](../images/desktop.png)

*螢幕擷取畫面使用內建的示範磁碟機；檔案名稱與大小皆為虛構。*

## 功能

瀏覽依大小排序的目錄樹與檔案類型統計。按兩下可放大，使用返回按鈕或 Escape 可返回。掃描期間也能操作地圖，支援暫停、繼續與即時更新。可以開啟檔案、檢視內容或移至資源回收筒，也可停用刪除功能。提供十種配色，可調整密度與大小計算方式，介面支援 31 種語言。

![Clawback](../images/explore.png)

## 安裝

下載最新版本。在 Windows 上執行 `clawback.exe`，在 macOS 上將 `Clawback.app` 拖到 Applications，在 Linux 上執行 `clawback`。接著選擇 **檔案 → 開啟資料夾**。從 crates.io 安裝需要 Rust 1.95 或更新版本，並使用系統的中文、日文與韓文字型；下載的發行版自帶這些字型。

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## 終端機

以下指令分別啟動桌面應用程式、互動式終端機地圖與文字報告。終端機支援瀏覽與即時更新，但不提供檔案操作。報告也可用於指令碼。

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## 掃描

在 Windows 上，Turbo 透過獨立的唯讀輔助程序更快地讀取整個本機 NTFS 磁碟區，並要求 UAC 授權。一般掃描仍可使用。macOS 不需管理員權限即可批次讀取中繼資料。目錄掃描器會自動調整執行緒數量。無法讀取的資料夾會列出；不會跟隨符號連結或 Windows 目錄接合點。

## 參與貢獻

建置說明與使用指南為英文。翻譯在 Git 中維護；CI 要求完整的翻譯目錄，以及根據目前英文來源審核過的 README。

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
