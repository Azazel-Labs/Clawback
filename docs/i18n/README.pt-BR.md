# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · Português (Brasil) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

O Clawback mostra o que ocupa o espaço do seu disco. Ele analisa uma unidade ou pasta e exibe os arquivos como retângulos proporcionais ao espaço usado, dentro de suas pastas. Funciona no Windows, macOS e Linux, com aplicativo de desktop e modo de terminal.

![Clawback](../images/desktop.png)

*Dados de demonstração: arquivos e tamanhos fictícios*

## Recursos

Explore a árvore ordenada por tamanho e o detalhamento por tipo de arquivo. Clique duas vezes para ampliar e use Voltar ou Escape para retornar. O mapa funciona durante a análise, com pausa, retomada e atualizações em tempo real. Abra arquivos, veja propriedades ou mova-os para a lixeira; a exclusão pode ser desativada. Dez paletas, densidade e cálculo de tamanhos ajustáveis, e interface em 31 idiomas.

![Clawback](../images/explore.png)

## Instalação

Baixe a versão mais recente. No Windows, execute `clawback.exe`; no macOS, arraste `Clawback.app` para Applications; no Linux, execute `clawback`. Selecione **Arquivo → Abrir pasta**. A instalação pelo crates.io exige Rust 1.95 ou mais recente e usa as fontes chinesas, japonesas e coreanas do sistema; as versões para download incluem essas fontes.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Os comandos abaixo abrem o aplicativo de desktop, o mapa interativo no terminal e um relatório de texto. O terminal permite navegação e atualizações em tempo real, mas não ações em arquivos. Os relatórios também funcionam em scripts.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Análise

No Windows, o Turbo lê um volume NTFS local inteiro mais rapidamente por meio de um processo auxiliar separado, somente de leitura, que solicita autorização do UAC. A análise normal continua disponível. O macOS lê metadados em lotes sem privilégios de administrador. O analisador de diretórios ajusta automaticamente suas threads. Pastas ilegíveis são informadas; links simbólicos e junções do Windows não são seguidos.

## Contribuições

As instruções de compilação e o guia do usuário estão em inglês. As traduções são mantidas no Git; a CI exige catálogos completos e READMEs revisados em relação às fontes atuais em inglês.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
