# Noto Sans SC, TC, KR, and JP

Clawback embeds the unmodified Noto Sans SC, TC, KR, and JP Regular 2.004 fonts as
fallbacks for proportional and monospace UI text, including Chinese, Korean, and Japanese
filenames and the native language names in every language's Settings dialog.

- Copyright © 2014–2021 Adobe (http://www.adobe.com/).
- License: SIL Open Font License 1.1; see [OFL.txt](OFL.txt).
- SC source: https://github.com/notofonts/noto-cjk/blob/Sans2.004/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf
- SC SHA-256: `faa6c9df652116dde789d351359f3d7e5d2285a2b2a1f04a2d7244df706d5ea9`
- TC source: https://github.com/notofonts/noto-cjk/blob/Sans2.004/Sans/SubsetOTF/TC/NotoSansTC-Regular.otf
- TC SHA-256: `5bab0cb3c1cf89dde07c4a95a4054b195afbcfe784d69d75c340780712237537`
- KR source: https://github.com/notofonts/noto-cjk/blob/Sans2.004/Sans/SubsetOTF/KR/NotoSansKR-Regular.otf
- KR SHA-256: `69975a0ac8472717870aefeab0a4d52739308d90856b9955313b2ad5e0148d68`
- JP source: https://github.com/notofonts/noto-cjk/blob/Sans2.004/Sans/SubsetOTF/JP/NotoSansJP-Regular.otf
- JP SHA-256: `dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073`

The fonts add approximately 8.3 MB (SC), 5.7 MB (TC), 4.6 MB (KR), and 4.5 MB (JP) to the uncompressed
executable. They need no runtime download or installed system font. Release
archives include the shared license as NotoSans-OFL.txt; macOS app bundles
also carry the license in Contents/Resources.

The crates.io package leaves these four fonts out to stay under its size limit, so
`cargo install` builds use the operating system's CJK fonts instead: Microsoft
YaHei, JhengHei, Yu Gothic, and Malgun Gothic on Windows; PingFang, Hiragino, and
Apple SD Gothic Neo on macOS; and whatever fontconfig picks for each language on
Linux (usually Noto Sans CJK). Linux systems without CJK fonts show boxes for those
characters. `build.rs` bundles the fonts whenever all four files are present.

Traditional Chinese, Japanese, and Korean UI languages prioritize their respective fonts for shared
Han characters. Other languages keep SC first. Switching languages updates
the fallback order; all four fonts remain available for mixed-script text.

## Noto Sans Thai

The unmodified Noto Sans Thai Regular 2.000 font is also embedded for both
families, including Thai native language names when another language is active.
It adds 37,752 bytes and needs no installed system font.

- Font copyright: Copyright 2016 Google Inc. All Rights Reserved.
- License: SIL Open Font License 1.1; see [NotoSansThai-OFL.txt](NotoSansThai-OFL.txt).
- Source: https://github.com/notofonts/noto-fonts/blob/main/hinted/ttf/NotoSansThai/NotoSansThai-Regular.ttf
- License source: https://github.com/notofonts/noto-fonts/blob/main/LICENSE
- SHA-256: `404ddfb5ed0aaa6b6ec8a85700d682978992062d67da93903967b56cbd9a4acc`

Release archives and macOS app Resources include `NotoSansThai-OFL.txt`.
