# Third-party licenses

Nib ships the following third-party components. The full license texts are
available from the linked upstream sources; short-form notices are reproduced
below.

## PDFium (`libpdfium.so`)

- Upstream: <https://pdfium.googlesource.com/pdfium/>
- Binaries: <https://github.com/bblanchon/pdfium-binaries>, release `chromium/8125`
- License: BSD-3-Clause

```
Copyright 2014 The Chromium Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Libraries bundled inside PDFium

| Library | License | License text |
|---|---|---|
| FreeType | FreeType License (FTL) / GPL-2.0 dual, FTL applies | <https://gitlab.freedesktop.org/freetype/freetype/-/blob/master/docs/FTL.TXT> |
| libjpeg-turbo | IJG / BSD-3-Clause | <https://github.com/libjpeg-turbo/libjpeg-turbo/blob/main/LICENSE.md> |
| OpenJPEG | BSD-2-Clause | <https://github.com/uclouvain/openjpeg/blob/master/LICENSE> |
| zlib | zlib | <https://github.com/madler/zlib/blob/master/LICENSE> |
| ICU | Unicode License (ICU) | <https://github.com/unicode-org/icu/blob/main/icu4c/LICENSE> |

## UniFFI

- Upstream: <https://github.com/mozilla/uniffi-rs>
- License: MPL-2.0 (<https://www.mozilla.org/MPL/2.0/>)

Nib uses UniFFI unmodified. The UniFFI runtime is compiled into
`libnib_ffi.so`; MPL-2.0 is file-scoped, so the Kotlin bindings UniFFI
generates and Nib's own sources are not MPL-covered. UniFFI source is
obtainable from the upstream link above.

## JNA (`net.java.dev.jna:jna`)

- Upstream: <https://github.com/java-native-access/jna>
- License: dual Apache-2.0 / LGPL-2.1-or-later; Nib elects **Apache-2.0**.

## pdfium-render (Rust crate)

- Upstream: <https://github.com/ajrcarey/pdfium-render>
- License: MIT OR Apache-2.0; Nib elects **Apache-2.0**.

## lopdf (Rust crate)

- Upstream: <https://github.com/J-F-Liu/lopdf>
- License: MIT (<https://opensource.org/licenses/MIT>)
