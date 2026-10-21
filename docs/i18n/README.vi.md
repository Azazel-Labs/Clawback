# Clawback

[![CI](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml/badge.svg)](https://github.com/Azazel-Labs/Clawback/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Azazel-Labs/Clawback)](https://github.com/Azazel-Labs/Clawback/releases)
[![crates.io](https://img.shields.io/crates/v/clawback.svg)](https://crates.io/crates/clawback)
[![License: MIT-0](https://img.shields.io/badge/license-MIT--0-blue.svg)](../../LICENSE)
[![Rust: 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](../../Cargo.toml)
[![Edition: 2024](https://img.shields.io/badge/edition-2024-orange.svg)](../../Cargo.toml)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](../../README.md#install)

[English](../../README.md) · [Afrikaans](README.af.md) · [Català](README.ca.md) · [Čeština](README.cs.md) · [Dansk](README.da.md) · [Deutsch](README.de.md) · [Ελληνικά](README.el.md) · [Español (Latinoamérica)](README.es-419.md) · [Español (España)](README.es-ES.md) · [Suomi](README.fi.md) · [Français](README.fr.md) · [Magyar](README.hu.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Norsk bokmål](README.nb.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Português (Brasil)](README.pt-BR.md) · [Português (Portugal)](README.pt-PT.md) · [Română](README.ro.md) · [Русский](README.ru.md) · [Српски](README.sr-Cyrl.md) · [Svenska](README.sv.md) · [ไทย](README.th.md) · [Türkçe](README.tr.md) · [Українська](README.uk.md) · Tiếng Việt · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md)

Clawback cho biết những gì đang chiếm dung lượng đĩa. Ứng dụng quét ổ đĩa hoặc thư mục và hiển thị tệp dưới dạng hình chữ nhật có kích thước theo dung lượng sử dụng, nằm trong thư mục của chúng. Hỗ trợ Windows, macOS và Linux, với ứng dụng đồ họa và chế độ terminal.

![Clawback](../images/desktop.png)

*Ảnh chụp màn hình dùng ổ đĩa minh họa tích hợp sẵn; tên và kích thước tệp là giả định.*

## Tính năng

Khám phá cây sắp xếp theo kích thước và thống kê theo loại tệp. Nhấp đúp để phóng to, dùng Quay lại hoặc Escape để trở về. Bản đồ dùng được ngay khi đang quét, hỗ trợ tạm dừng, tiếp tục và cập nhật trực tiếp. Mở tệp, xem thuộc tính hoặc chuyển vào thùng rác; có thể tắt chức năng xóa. Mười bảng màu, điều chỉnh mật độ và cách tính kích thước, giao diện 31 ngôn ngữ.

![Clawback](../images/explore.png)

## Cài đặt

Tải bản phát hành mới nhất. Trên Windows chạy `clawback.exe`, trên macOS kéo `Clawback.app` vào Applications, trên Linux chạy `clawback`. Chọn **Tệp → Mở thư mục**. Cài từ crates.io yêu cầu Rust 1.95 trở lên và dùng phông chữ Trung, Nhật và Hàn của hệ thống; các bản tải sẵn có kèm những phông chữ này.

[GitHub Releases](https://github.com/Azazel-Labs/Clawback/releases/latest) · [crates.io](https://crates.io/crates/clawback)

```sh
cargo install clawback
```

## Terminal

Các lệnh dưới đây mở ứng dụng đồ họa, bản đồ terminal tương tác và báo cáo văn bản. Terminal hỗ trợ điều hướng và cập nhật trực tiếp nhưng không hỗ trợ thao tác tệp. Báo cáo cũng dùng được trong tập lệnh.

```sh
clawback
clawback --tui .
clawback --report --top 10 .
```

![Clawback](../images/terminal.png)

## Quét

Trên Windows, Turbo đọc toàn bộ ổ NTFS cục bộ nhanh hơn qua một tiến trình trợ giúp riêng chỉ đọc và yêu cầu chấp thuận UAC. Chế độ quét thường vẫn khả dụng. macOS đọc siêu dữ liệu theo lô mà không cần quyền quản trị. Bộ quét thư mục tự điều chỉnh số luồng. Các thư mục không đọc được sẽ được báo cáo; không đi theo liên kết tượng trưng hay junction của Windows.

## Đóng góp

Hướng dẫn biên dịch và hướng dẫn sử dụng bằng tiếng Anh. Bản dịch được quản lý trong Git; CI yêu cầu danh mục đầy đủ và README đã được kiểm tra theo nguồn tiếng Anh hiện tại.

[docs/usage.md](../usage.md) · [docs/development.md](../development.md) · [docs/translations.md](../translations.md) · [docs/releases.md](../releases.md)

Copyright © 2026 Azazel Labs. [MIT-0](../../LICENSE).
