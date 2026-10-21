# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · 简体中文 · [繁體中文](README.zh-Hant.md)

Clawback 帮你了解磁盘空间被什么占用。它扫描驱动器或文件夹，将文件显示为面积与占用空间成比例的矩形，并按文件夹层级嵌套排列。支持 Windows、macOS 和 Linux，提供桌面应用和终端模式。

![Clawback](../images/desktop.png)

*截图使用内置的演示驱动器；文件名和大小均为虚构。*

## 功能

浏览按大小排序的目录树和文件类型统计。双击可放大，使用返回按钮或 Escape 可返回。扫描期间也能操作地图，支持暂停、继续和实时更新。可以打开文件、查看属性或移到回收站，也可禁用删除功能。提供十种配色方案，可调整密度和大小计算方式，界面支持 31 种语言。

![Clawback](../images/explore.png)

## 安装

下载最新版本。在 Windows 上运行 `clawback.exe`，在 macOS 上将 `Clawback.app` 拖到 Applications，在 Linux 上运行 `clawback`。然后选择 **文件 → 打开文件夹**。从 crates.io 安装需要 Rust 1.95 或更新版本，并使用系统的中文、日文和韩文字体；下载的发行版自带这些字体。

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## 终端

以下命令分别启动桌面应用、交互式终端地图和文本报告。终端支持浏览和实时更新，但不提供文件操作。报告也可用于脚本。

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## 扫描

在 Windows 上，Turbo 通过独立的只读辅助进程更快地读取整个本地 NTFS 卷，并请求 UAC 授权。普通扫描仍然可用。macOS 无需管理员权限即可批量读取元数据。目录扫描器会自动调整线程数。无法读取的文件夹会被列出；不会跟随符号链接或 Windows 目录联接。

## 参与贡献

构建说明和用户指南为英文。翻译在 Git 中维护；CI 要求完整的翻译目录，以及根据当前英文源文审核过的 README。

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
