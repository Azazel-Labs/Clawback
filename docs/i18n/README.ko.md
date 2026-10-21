# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · 한국어 · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback은 디스크 공간을 무엇이 차지하는지 보여 줍니다. 드라이브나 폴더를 검사하고, 파일을 사용량에 비례하는 직사각형으로 폴더 안에 표시합니다. Windows, macOS, Linux에서 데스크톱 앱과 터미널 모드로 사용할 수 있습니다.

![Clawback](../images/desktop.png)

*데모 데이터 - 가상 파일 및 크기*

## 기능

크기순 트리와 파일 형식별 내역을 탐색하세요. 두 번 클릭하면 확대되고, 뒤로 또는 Escape로 돌아갑니다. 검사 중에도 지도를 사용할 수 있으며 일시 정지, 재개, 실시간 업데이트를 지원합니다. 파일 열기, 속성 보기, 휴지통으로 이동하기가 가능하고 삭제 기능은 끌 수 있습니다. 색상표 10개, 밀도와 크기 계산 설정, 31개 언어 인터페이스를 제공합니다.

![Clawback](../images/explore.png)

## 설치

최신 릴리스를 다운로드하세요. Windows에서는 `clawback.exe`를 실행하고, macOS에서는 `Clawback.app`을 Applications로 드래그하고, Linux에서는 `clawback`을 실행합니다. **파일 → 폴더 열기**를 선택하세요. crates.io에서 설치하려면 Rust 1.95 이상이 필요하며 시스템의 중국어·일본어·한국어 글꼴을 사용합니다. 다운로드용 릴리스에는 글꼴이 포함되어 있습니다.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## 터미널

아래 명령은 각각 데스크톱 앱, 대화형 터미널 지도, 텍스트 보고서를 실행합니다. 터미널은 탐색과 실시간 업데이트를 지원하지만 파일 작업은 지원하지 않습니다. 보고서는 스크립트에서도 사용할 수 있습니다.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## 검사

Windows의 Turbo는 UAC 승인을 요청하는 별도의 읽기 전용 도우미 프로세스로 로컬 NTFS 볼륨 전체를 더 빠르게 읽습니다. 일반 검사도 계속 사용할 수 있습니다. macOS는 관리자 권한 없이 메타데이터를 일괄 읽습니다. 디렉터리 검사기는 스레드 수를 자동 조정합니다. 읽을 수 없는 폴더는 보고하며 심볼릭 링크와 Windows 정션은 따라가지 않습니다.

## 기여

빌드 지침과 사용자 가이드는 영어로 제공됩니다. 번역은 Git에서 관리하며, CI는 빠짐없는 카탈로그와 현재 영어 원본에 맞춰 검토된 README를 요구합니다.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
