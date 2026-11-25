# Font test fixtures

These reduced TrueType fixtures are derived from the Google Fonts distributions
of [Noto Sans SC](https://github.com/google/fonts/tree/main/ofl/notosanssc) and
[Noto Sans Symbols 2](https://github.com/google/fonts/tree/main/ofl/notosanssymbols2).
The source font metadata versions are `2.004-H2` and `2.008`, respectively.
Both are distributed under the SIL Open Font License 1.1; the copyright
notices and full license are in [OFL.txt](./OFL.txt).

The files are deliberately small test subsets, not production fonts:

| File | Source family | Included Unicode code points |
|---|---|---|
| `noto-sans-sc-subset.ttf` | Noto Sans SC | U+0020, U+0041, U+004C, U+004E, U+0053, U+0061, U+0063, U+0066, U+0069, U+006E, U+006F, U+0073, U+0074, U+00A0, U+00C5, U+00E9, U+041F, U+0432, U+0435, U+0438, U+043C, U+0440, U+0442, U+2014, U+212B, U+4E2D, U+6587 |
| `noto-sans-symbols2-subset.ttf` | Noto Sans Symbols 2 | U+0020, U+0041, U+1F4DA |

They were generated from the corresponding upstream TrueType sources with
FontTools `pyftsubset`, preserving Unicode cmap and OS/2 tables for production
font parsing and embedding-rights tests. To regenerate, use the matching
upstream source version and these code points:

```sh
pyftsubset SOURCE.ttf --unicodes="U+0020,U+0041,U+004C,U+004E,U+0053,U+0061,U+0063,U+0066,U+0069,U+006E,U+006F,U+0073,U+0074,U+00A0,U+00C5,U+00E9,U+041F,U+0432,U+0435,U+0438,U+043C,U+0440,U+0442,U+2014,U+212B,U+4E2D,U+6587" \
  --output-file=noto-sans-sc-subset.ttf
pyftsubset SYMBOLS_SOURCE.ttf --unicodes="U+0020,U+0041,U+1F4DA" \
  --output-file=noto-sans-symbols2-subset.ttf
```

`subsetter` intentionally strips the source cmap and OS/2 tables when producing
PDF subsets. These checked-in inputs retain those tables so tests can exercise
font lookup, rights validation, and production subsetting from a realistic
source font.
