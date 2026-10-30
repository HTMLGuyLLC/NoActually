# Third-party notices

"No, Actually..." is MIT licensed (see [LICENSE](LICENSE)). It bundles the fonts and depends on the packages below. This file lists them and their licenses.

## Bundled fonts

The app ships these fonts inside the installer, so nothing is fetched from the network. They come from [Fontsource](https://fontsource.org/) and are licensed under the SIL Open Font License 1.1 (full text below). None of them is modified.

| Font | Package | Copyright |
|---|---|---|
| Bricolage Grotesque | `@fontsource-variable/bricolage-grotesque` 5.3.0 | Copyright 2022 The Bricolage Grotesque Project Authors (https://github.com/ateliertriay/bricolage) |
| Instrument Sans | `@fontsource/instrument-sans` 5.3.0 | Copyright 2022 The Instrument Sans Project Authors (https://github.com/Instrument/instrument-sans) |
| Gochi Hand | `@fontsource/gochi-hand` 5.3.0 | Copyright (c) 2011, HT Fonts (www.htfonts.com info@htfonts.com), with Reserved Font Names "Gochi" and "Gochi Hand" |
| JetBrains Mono | `@fontsource/jetbrains-mono` 5.3.0 | Copyright 2020 The JetBrains Mono Project Authors (https://github.com/JetBrains/JetBrainsMono) |

### SIL Open Font License 1.1

This Font Software is licensed under the SIL Open Font License, Version 1.1. This license is copied below, and is also available with a FAQ at: https://openfontlicense.org

```
-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```

## Rust crates

These are the direct runtime dependencies from the `Cargo.toml` files (versions as locked in `Cargo.lock`). Each crate's full license text ships in its own package. Their own dependencies are covered by their packages in the same way.

| Crate | License |
|---|---|
| `tauri` 2.12.5 and the `tauri-plugin-*` crates (notification, autostart, positioner, dialog, single-instance, log, opener, updater) | Apache-2.0 OR MIT |
| `regex`, `serde`, `serde_json`, `chrono`, `anyhow`, `dirs`, `rayon`, `sha2`, `log` | MIT OR Apache-2.0 |
| `thiserror` | MIT OR Apache-2.0 |
| `memchr` | Unlicense OR MIT |
| `ruzstd`, `rusqlite` | MIT |
| `notify` | CC0-1.0 |

`rusqlite` is built with the `bundled` feature, so it includes SQLite, which is in the public domain.

## npm packages

The only runtime npm dependency besides the fonts above is `@tauri-apps/api` 2.12.4 (Apache-2.0 OR MIT). Build and test tools (Vite, Vitest, TypeScript, Biome, the Tauri CLI) are not shipped in the app.

## Full list

Run `cargo tree` for the full Rust dependency tree and `pnpm --dir app list --prod` for the npm one. `cargo deny check licenses` (config in [`deny.toml`](deny.toml)) checks that every Rust dependency uses an allowed license.
