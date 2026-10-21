# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · ไทย · [Türkçe](README.tr.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback แสดงว่าอะไรใช้พื้นที่ดิสก์ของคุณ โดยสแกนไดรฟ์หรือโฟลเดอร์และแสดงไฟล์เป็นสี่เหลี่ยมที่มีขนาดตามพื้นที่ที่ใช้ ซ้อนอยู่ภายในโฟลเดอร์ รองรับ Windows, macOS และ Linux ทั้งแอปเดสก์ท็อปและโหมดเทอร์มินัล

![Clawback](../images/desktop.png)

*ภาพหน้าจอใช้ไดรฟ์สาธิตที่มีในแอป ชื่อไฟล์และขนาดเป็นข้อมูลสมมติ*

## คุณสมบัติ

สำรวจโครงสร้างที่เรียงตามขนาดและสรุปตามชนิดไฟล์ ดับเบิลคลิกเพื่อขยาย และใช้ปุ่มย้อนกลับหรือ Escape เพื่อกลับ ใช้แผนที่ได้ระหว่างสแกน พร้อมหยุดชั่วคราว ทำต่อ และอัปเดตแบบสด เปิดไฟล์ ดูคุณสมบัติ หรือย้ายไปถังขยะได้ และปิดการลบได้ มีชุดสีสิบชุด ปรับความหนาแน่นและวิธีคำนวณขนาดได้ พร้อมส่วนติดต่อ 31 ภาษา

![Clawback](../images/explore.png)

## การติดตั้ง

ดาวน์โหลดรุ่นล่าสุด บน Windows เรียกใช้ `clawback.exe` บน macOS ลาก `Clawback.app` ไปที่ Applications และบน Linux เรียกใช้ `clawback` จากนั้นเลือก **ไฟล์ → เปิดโฟลเดอร์** การติดตั้งจาก crates.io ต้องใช้ Rust 1.95 ขึ้นไป และใช้แบบอักษรจีน ญี่ปุ่น และเกาหลีของระบบ ส่วนรุ่นที่ดาวน์โหลดมีแบบอักษรเหล่านี้มาให้

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## เทอร์มินัล

คำสั่งด้านล่างเปิดแอปเดสก์ท็อป แผนที่เทอร์มินัลแบบโต้ตอบ และรายงานข้อความตามลำดับ เทอร์มินัลรองรับการนำทางและอัปเดตแบบสด แต่ไม่รองรับการดำเนินการกับไฟล์ รายงานใช้ในสคริปต์ได้ด้วย

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## การสแกน

บน Windows โหมด Turbo อ่านทั้งโวลุ่ม NTFS ภายในเครื่องได้เร็วขึ้นผ่านกระบวนการช่วยแยกต่างหากที่อ่านอย่างเดียวและขออนุมัติ UAC โดยยังใช้การสแกนปกติได้ macOS อ่านข้อมูลเมตาเป็นชุดโดยไม่ต้องใช้สิทธิ์ผู้ดูแลระบบ ตัวสแกนไดเรกทอรีปรับจำนวนเธรดโดยอัตโนมัติ มีรายงานโฟลเดอร์ที่อ่านไม่ได้ และไม่ติดตามลิงก์สัญลักษณ์หรือจังก์ชันของ Windows

## การมีส่วนร่วม

คำแนะนำการบิลด์และคู่มือผู้ใช้เป็นภาษาอังกฤษ จัดการคำแปลใน Git โดย CI กำหนดให้แค็ตตาล็อกครบถ้วนและ README ผ่านการตรวจเทียบกับต้นฉบับภาษาอังกฤษปัจจุบัน

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
