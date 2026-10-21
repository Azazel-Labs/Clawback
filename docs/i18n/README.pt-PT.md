# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · Português (Portugal) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

O Clawback mostra o que ocupa o espaço do seu disco. Analisa uma unidade ou pasta e apresenta os ficheiros como retângulos proporcionais ao espaço utilizado, dentro das respetivas pastas. Funciona em Windows, macOS e Linux, com aplicação gráfica e modo de terminal.

![Clawback](../images/desktop.png)

*Dados de demonstração: ficheiros e tamanhos fictícios*

## Funcionalidades

Explore a árvore ordenada por tamanho e a distribuição por tipo de ficheiro. Faça duplo clique para ampliar e use Voltar ou Escape para regressar. O mapa funciona durante a análise, com pausa, retoma e atualizações em tempo real. Abra ficheiros, consulte propriedades ou mova-os para a reciclagem; a eliminação pode ser desativada. Dez paletas, densidade e cálculo de tamanhos ajustáveis, e interface em 31 idiomas.

![Clawback](../images/explore.png)

## Instalação

Transfira a versão mais recente. Em Windows, execute `clawback.exe`; em macOS, arraste `Clawback.app` para Applications; em Linux, execute `clawback`. Selecione **Ficheiro → Abrir pasta**. A instalação através do crates.io requer Rust 1.95 ou posterior e utiliza os tipos de letra chineses, japoneses e coreanos do sistema; as versões prontas a usar incluem-nos.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Os comandos abaixo abrem a aplicação gráfica, o mapa interativo no terminal e um relatório de texto. O terminal permite navegação e atualizações em tempo real, mas não operações sobre ficheiros. Os relatórios também funcionam em scripts.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Análise

Em Windows, o Turbo lê mais depressa um volume NTFS local completo através de um processo auxiliar separado, só de leitura, que solicita autorização UAC. A análise normal continua disponível. O macOS lê metadados em lotes sem privilégios de administrador. O analisador de diretórios ajusta automaticamente as threads. As pastas ilegíveis são indicadas; não são seguidas ligações simbólicas nem junções do Windows.

## Contribuições

As instruções de compilação e o guia do utilizador estão em inglês. As traduções são mantidas no Git; a CI exige catálogos completos e READMEs revistos face às fontes atuais em inglês.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
