# Nib PDF — Delta State & Serialization Specification

> Status: **Schema v1 (draft)**. `schemaVersion` is bumped only on breaking changes;
> additive fields are always allowed and must be preserved by implementations
> (`#[serde(flatten)] extra`). Companion documents: [`ARCHITECTURE.md`](./ARCHITECTURE.md),
> [`ROADMAP.md`](./ROADMAP.md). A machine-readable JSON Schema (draft 2020-12) mirroring
> this document lives at `nib-rs/nib-core/schema/nib-delta.schema.json` (created in M2).

---

## 1. Design principles

1. **XFDF semantics, JSON syntax.** Field and annotation concepts map 1:1 onto PDF (ISO 32000-1 §12.5 and §12.7) so that any delta can be written to a PDF without loss, and any PDF annotation can be exported as a delta.
2. **User-space geometry only.** All coordinates are PDF user-space points, y-up, relative to the page's `/CropBox` origin as normalised by `PageTransform`. Never device pixels, never dp.
3. **Stable identity.** Every entity has a UUID v4 `id` that is written to the PDF (`/NM` for annotations, fully-qualified `/T` for fields). Ops reference entities only by `id`.
4. **Ops are the unit of history.** `create` / `update` / `delete` are the only verbs. `update` carries a JSON Merge Patch (RFC 7386) so partial edits are compact and the inverse is derivable.
5. **Self-describing & forward-compatible.** `schemaVersion`, `subtype`/`fieldType` discriminators, unknown keys preserved.
6. **Rust-first typing.** Every schema below has a `serde` struct/enum in `nib-core::model`; the JSON is the serialization of that type (`rename_all = "camelCase"`, `tag = "subtype"` for annotations).

Conventions: colours are `"#RRGGBB"` strings (no alpha — use `opacity`); `opacity` ∈ `[0,1]`; timestamps are RFC 3339 UTC; lengths/widths in points; `rect` is `[x0, y0, x1, y1]` with `x0<x1`, `y0<y1`; a `point` is `[x, y]`.

---

## 2. Envelope

`ops` is the `Op[]` in application order:

```json
{
  "schemaVersion": 1,
  "documentId": "sha256:3f1c…9a",
  "pdfId": ["<ID[0] hex>", "<ID[1] hex>"],
  "baseRevision": 0,
  "generator": "nib-pdf/0.1.0 (android)",
  "createdAt": "2026-11-12T14:03:22Z",
  "ops": []
}
```

| Field | Type | Req | Meaning |
|---|---|---|---|
| `schemaVersion` | int | yes | `1` |
| `documentId` | string | yes | `"sha256:" + hex(SHA-256(revision-0 bytes))`. Import refuses a mismatch unless `force=true`. |
| `pdfId` | `[string, string]` | no | Trailer `/ID` pair, for human diagnostics. |
| `baseRevision` | int | yes | Revision index the ops were recorded against (0 = file as opened). |
| `generator` | string | no | |
| `createdAt` | string | no | |
| `ops` | `Op[]` | yes | Ordered. |

```rust
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct DeltaDocument {
    pub schema_version: u32,
    pub document_id: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub pdf_id: Option<[String; 2]>,
    pub base_revision: u32,
    #[serde(default)] pub generator: Option<String>,
    #[serde(default)] pub created_at: Option<DateTime<Utc>>,
    pub ops: Vec<Op>,
    #[serde(flatten)] pub extra: serde_json::Map<String, Value>,
}
```

---

## 3. Operations

```json
{
  "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c44",
  "ts": "2026-11-12T14:03:25.120Z",
  "author": "Andrea",
  "groupId": "0193b2e4-7c1a-7f2e-9a53-000000000001",
  "type": "update",
  "target": "annotation",
  "id": "9b3a6c1e-1d2f-4e0a-9e4f-7a2c0b5d1f10",
  "page": 0,
  "payload": { "color": "#FF0000", "strokeWidth": 2.5 }
}
```

| Field | Type | Req | Notes |
|---|---|---|---|
| `opId` | UUID (v7 recommended) | yes | Idempotency key; duplicates are skipped on import. |
| `ts` | RFC 3339 | yes | |
| `author` | string | no | |
| `groupId` | UUID | no | Ops sharing a `groupId` undo/redo as one unit (transaction). |
| `type` | `"create" \| "update" \| "delete"` | yes | |
| `target` | `"annotation" \| "formField" \| "signature"` | yes | Selects the payload schema family. |
| `id` | UUID / field name | yes | Entity id. For `formField`, the fully-qualified `/T` name (e.g. `"applicant.name"`). |
| `page` | int (0-based) | create: yes; others: no | Denormalised for fast routing/invalidation. |
| `payload` | object | create: full entity; update: merge patch; delete: `{}` or `{ "reason": "…" }` | |

Payload semantics by `type`:

- **create** — `payload` is the complete entity (§4–§7). Fails if `id` exists (unless identical → treated as idempotent no-op).
- **update** — `payload` is an RFC 7386 merge patch applied to the current entity: keys set to `null` delete the key; nested objects merge recursively; arrays replace wholesale. The entity's `modifiedAt` is set by the engine. Fails if `id` is unknown.
- **delete** — removes the entity (and, for annotations, its `/Popup`). Fails if `id` is unknown.

```rust
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct Op {
    pub op_id: Uuid, pub ts: DateTime<Utc>,
    #[serde(default)] pub author: Option<String>,
    #[serde(default)] pub group_id: Option<Uuid>,
    #[serde(rename = "type")] pub kind: OpKind,            // Create | Update | Delete
    pub target: Target,                                     // Annotation | FormField | Signature
    pub id: String,
    #[serde(default)] pub page: Option<u32>,
    pub payload: serde_json::Value,                         // validated against the target schema on apply
}
```

Validation on apply (`nib-core::delta::validate`) returns a list of `ValidationError { opIndex, path, code, message }` with codes: `UNKNOWN_ID`, `DUPLICATE_ID`, `SCHEMA`, `PAGE_OUT_OF_RANGE`, `RECT_INVALID`, `SIGNATURE_LOCKED`, `FIELD_READONLY`, `VALUE_NOT_IN_OPTIONS`, `MAXLEN_EXCEEDED`, `REVISION_MISMATCH`.

---

## 4. Annotation base

All annotations share these keys (mapping to ISO 32000-1 Table 164):

| Key | Type | PDF | Notes |
|---|---|---|---|
| `id` | UUID | `/NM` | |
| `subtype` | enum | `/Subtype` | Discriminator: `Ink`, `Highlight`, `Underline`, `StrikeOut`, `Squiggly`, `FreeText`, `Text`, `Stamp`, `Square`, `Circle`, `Line` |
| `page` | int | parent `/Annots` | |
| `rect` | `[x0,y0,x1,y1]` | `/Rect` | Engine may recompute from geometry (`Ink`, quads, `Line`) when `rect` is omitted on create. |
| `color` | `#RRGGBB \| null` | `/C` | Stroke/text colour; `null` = transparent. |
| `opacity` | number | `/CA` + ExtGState | default `1` |
| `author` | string | `/T` | |
| `contents` | string | `/Contents` | Note text / FreeText body / alt text. |
| `subject` | string | `/Subj` | |
| `createdAt` | RFC 3339 | `/CreationDate` | |
| `modifiedAt` | RFC 3339 | `/M` | Engine-maintained. |
| `flags` | object | `/F` | `{ "hidden": false, "print": true, "locked": false, "readOnly": false, "noView": false }` → bits 2,3,8,7,6 |
| `inReplyTo` | UUID | `/IRT` | Reply threads (`/RT /R`). |
| `popup` | `{ "open": bool, "rect": [...] }` | `/Popup` | Optional. |
| `appearance` | object | `/AP` | `{ "regenerate": true }` (default) tells the writer to (re)build `/AP`; `{ "regenerate": false, "apHash": "sha256:…" }` preserves an imported, foreign `/AP`. |
| `custom` | object | `/NibData` (JSON string) | Free-form host data, round-tripped. |

```rust
#[derive(Serialize, Deserialize)] #[serde(tag = "subtype", rename_all = "camelCase")]
pub enum Annotation { Ink(InkAnnot), Highlight(MarkupAnnot), Underline(MarkupAnnot),
    StrikeOut(MarkupAnnot), Squiggly(MarkupAnnot), FreeText(FreeTextAnnot), Text(TextAnnot),
    Stamp(StampAnnot), Square(ShapeAnnot), Circle(ShapeAnnot), Line(LineAnnot) }

#[derive(Serialize, Deserialize, Default)] #[serde(rename_all = "camelCase")]
pub struct AnnotBase {
    pub id: Uuid, pub page: u32,
    #[serde(default)] pub rect: Option<Rect>,
    #[serde(default)] pub color: Option<Color>,
    #[serde(default = "one")] pub opacity: f32,
    #[serde(default)] pub author: Option<String>,
    #[serde(default)] pub contents: Option<String>,
    #[serde(default)] pub subject: Option<String>,
    #[serde(default)] pub created_at: Option<DateTime<Utc>>,
    #[serde(default)] pub modified_at: Option<DateTime<Utc>>,
    #[serde(default)] pub flags: AnnotFlags,
    #[serde(default)] pub in_reply_to: Option<Uuid>,
    #[serde(default)] pub popup: Option<Popup>,
    #[serde(default)] pub appearance: Appearance,
    #[serde(default)] pub custom: serde_json::Map<String, Value>,
    #[serde(flatten)] pub extra: serde_json::Map<String, Value>,
}
```

---

## 5. Annotation subtypes

### 5.1 `Ink` (freehand, handwritten signatures)

```json
{
  "id": "9b3a6c1e-1d2f-4e0a-9e4f-7a2c0b5d1f10",
  "subtype": "Ink",
  "page": 0,
  "rect": [72.0, 500.2, 231.7, 560.9],
  "color": "#1A237E",
  "opacity": 1.0,
  "strokeWidth": 2.0,
  "lineCap": "round",
  "lineJoin": "round",
  "pressureSensitive": true,
  "minWidthFactor": 0.4,
  "maxWidthFactor": 1.6,
  "smoothing": {
    "algorithm": "catmullRom",
    "tension": 0.5,
    "resampleSpacing": 1.5,
    "simplifyTolerance": 0.3
  },
  "strokes": [
    {
      "points": [[74.1, 540.0, 0.31, 0], [75.0, 541.8, 0.42, 9], [76.9, 544.1, 0.55, 17]],
      "tool": "stylus",
      "startedAt": "2026-11-12T14:03:24.980Z"
    }
  ],
  "intent": null
}
```

| Key | Type | PDF | Notes |
|---|---|---|---|
| `strokeWidth` | number | `/BS /W` | Base width in points. |
| `lineCap` / `lineJoin` | `"round" \| "butt" \| "square"` / `"round" \| "miter" \| "bevel"` | `/AP` (`J`, `j`) | Default round/round. |
| `pressureSensitive` | bool | `/AP` only | When true the AP renders variable-width outlines using `width = strokeWidth · lerp(minWidthFactor, maxWidthFactor, pressure)`. |
| `smoothing.algorithm` | `"none" \| "catmullRom" \| "bezierFit"` | `/AP` only | `catmullRom`: tension (0..1); `bezierFit`: least-squares fit with `maxError` (pt). `/InkList` always gets raw resampled points. |
| `smoothing.resampleSpacing` | number | | Minimum distance between stored points after resampling. |
| `smoothing.simplifyTolerance` | number | | RDP tolerance applied before storage; `0` disables. |
| `strokes[].points` | `[x, y, pressure?, tMs?][]` | `/InkList` (x,y only) | `pressure` ∈ [0,1], `tMs` ms since `startedAt`. Fixed arity per stroke (2, 3 or 4). |
| `strokes[].tool` | `"stylus" \| "finger" \| "mouse"` | — | |
| `intent` | `null \| "signature" \| "initials"` | `/NibData` (no standard `/IT` value exists for ink signatures) | Marks the stroke set as a handwritten signature (see §6.1). |

Point appends during drawing are **not** emitted as ops (they'd be hundreds per second); the whole stroke set is emitted on gesture end. An `update` that adds a stroke replaces the `strokes` array (arrays replace wholesale), which is acceptable because strokes are small.

### 5.2 Text markup: `Highlight`, `Underline`, `StrikeOut`, `Squiggly`

```json
{
  "id": "c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a55",
  "subtype": "Highlight",
  "page": 2,
  "color": "#FFEB3B",
  "opacity": 0.5,
  "blendMode": "multiply",
  "quadPoints": [
    [100.0, 700.5, 300.2, 700.5, 100.0, 688.0, 300.2, 688.0],
    [72.0, 686.0, 180.4, 686.0, 72.0, 673.5, 180.4, 673.5]
  ],
  "contents": "Key clause",
  "selectedText": "…the party of the first part…"
}
```

| Key | Type | PDF | Notes |
|---|---|---|---|
| `quadPoints` | `number[8][]` | `/QuadPoints` | Order per ISO: `x1 y1 x2 y2 x3 y3 x4 y4` = upper-left, upper-right, lower-left, lower-right. Engine normalises. |
| `blendMode` | `"multiply" \| "normal"` | ExtGState `/BM` | Highlight default `multiply`; ignored for others. |
| `selectedText` | string | `/NibData` | Snapshot for search/accessibility; not authoritative. |
| `lineWidthFactor` | number | `/AP` | Underline/StrikeOut thickness as fraction of quad height (default 0.06). |

### 5.3 `FreeText`

```json
{
  "id": "4e1b0f3a-8c2d-4a6e-b1d0-9f8e7c6b5a43",
  "subtype": "FreeText",
  "page": 0,
  "rect": [300, 600, 500, 660],
  "contents": "Please initial here",
  "color": "#000000",
  "fontFamily": "Helvetica",
  "fontSize": 12,
  "fontWeight": "normal",
  "fontStyle": "normal",
  "textAlign": "left",
  "verticalAlign": "top",
  "lineHeight": 1.2,
  "padding": 2,
  "fillColor": "#FFFFFF",
  "borderColor": "#000000",
  "borderWidth": 1,
  "borderStyle": { "style": "solid", "dash": null },
  "rotation": 0,
  "callout": null
}
```

| Key | PDF | Notes |
|---|---|---|
| `fontFamily` | `/DA` font name; `/Resources /Font` | `"Helvetica" \| "Times" \| "Courier"` map to Base-14 (`/Helv`, `/TiRo`, `/Cour`); any other value triggers TTF subset embedding from a host-supplied font registry (`FontProvider` callback) with fallback to Helvetica. |
| `fontSize`, `color` | `/DA` (`size Tf`, `r g b rg`) | |
| `textAlign` | `/Q` (0/1/2) | |
| `verticalAlign`, `lineHeight`, `padding` | `/AP` only | |
| `fillColor` | `/IC` + `/AP` | ISO 32000 does not define a fill key for FreeText; Acrobat uses `/IC` by convention, so we write it and bake the fill into `/AP`. |
| `borderColor`, `borderWidth`, `borderStyle` | `/C` (border colour per spec), `/BS` | Note: for FreeText the spec's `/C` is the **border** colour; text colour is in `/DA`. |
| `rotation` | `/Rotate` (0/90/180/270) | Applied via `/Matrix`. |
| `callout` | `/IT /FreeTextCallout`, `/CL [x1 y1 x2 y2 (x3 y3)]`, `/LE` | `{ "points": [[...],[...],[...]], "lineEnding": "openArrow" }` |

### 5.4 `Text` (sticky note)

```json
{
  "id": "7d2c9b1a-3e4f-4c5d-a6b7-8c9d0e1f2a3b",
  "subtype": "Text",
  "page": 1,
  "rect": [540, 720, 560, 740],
  "color": "#FFC107",
  "icon": "comment",
  "open": false,
  "state": null,
  "contents": "Check this figure",
  "author": "Andrea",
  "popup": { "open": false, "rect": [560, 640, 760, 740] }
}
```

`icon` ∈ `comment | key | note | help | newParagraph | paragraph | insert` → `/Name`. `state` ∈ `null | accepted | rejected | cancelled | completed | marked | unmarked` → `/State` + `/StateModel` (review/marked). `/F` always includes NoZoom+NoRotate.

### 5.5 `Stamp`

```json
{
  "id": "a1b2c3d4-e5f6-4789-a012-b3c4d5e6f789",
  "subtype": "Stamp",
  "page": 0,
  "rect": [400, 80, 560, 160],
  "opacity": 1,
  "rotation": 0,
  "kind": "image",
  "image": {
    "assetId": "img-5a0d…",
    "mime": "image/png",
    "sha256": "sha256:9c0e…",
    "widthPx": 1200, "heightPx": 600,
    "dataBase64": null
  },
  "vector": null,
  "standardName": null,
  "preserveAspect": true
}
```

| `kind` | Payload | PDF |
|---|---|---|
| `"image"` | `image` with either `dataBase64` inline (≤ 1 MB) or `assetId` resolved via the host `AssetProvider` callback; `sha256` always present for dedup | Image XObject (`/DCTDecode` for JPEG passthrough, `/FlateDecode` + `/SMask` for PNG) referenced from `/AP` |
| `"vector"` | `vector: { "paths": [{ "d": "M 0 0 L 10 10 …", "fill": "#…", "stroke": "#…", "strokeWidth": n }], "viewBox": [0,0,w,h] }` (SVG-path-like mini-language: `M L C Z` only) | Content stream in `/AP`, scaled to `/Rect` |
| `"standard"` | `standardName` ∈ `Approved | Draft | Confidential | Final | Void | Completed | …` | `/Name /Approved…` + our own `/AP` (never rely on viewer stock art) |

### 5.6 Shapes: `Square`, `Circle`

```json
{
  "id": "0f9e8d7c-6b5a-4433-9221-0a1b2c3d4e5f",
  "subtype": "Square",
  "page": 3,
  "rect": [100, 100, 300, 200],
  "color": "#D32F2F",
  "fillColor": null,
  "opacity": 1,
  "strokeWidth": 2,
  "borderStyle": { "style": "dashed", "dash": [4, 2] },
  "cloudy": null
}
```

`fillColor` → `/IC`; `strokeWidth` → `/BS /W`; `borderStyle.style` ∈ `solid | dashed | beveled | inset | underline` → `/BS /S`; `dash` → `/BS /D`. `cloudy: { "intensity": 1|2 }` → `/BE` (v1 renders as plain border in `/AP`; reserved).

### 5.7 `Line`

```json
{
  "id": "5e4d3c2b-1a09-4f8e-b7d6-c5b4a3928170",
  "subtype": "Line",
  "page": 3,
  "start": [100, 400],
  "end": [300, 450],
  "color": "#000000",
  "strokeWidth": 1.5,
  "lineEndings": ["none", "closedArrow"],
  "fillColor": "#000000",
  "leaderLength": 0,
  "leaderExtension": 0,
  "caption": null
}
```

`start`/`end` → `/L`; `lineEndings` ∈ `none | square | circle | diamond | openArrow | closedArrow | butt | rOpenArrow | rClosedArrow | slash` → `/LE`; `fillColor` fills closed endings (`/IC`); `leaderLength`/`leaderExtension` → `/LL`/`/LLE`; `caption: { "text": "…", "position": "inline"|"top" }` → `/Cap`, `/CP`.

---

## 6. Signatures

### 6.1 Visual e-signature (no cryptography)

A visual signature is an ordinary `Ink` or `Stamp` annotation with `intent: "signature" | "initials"` plus signer metadata, optionally bound to a signature field widget so the mark lands exactly in the "sign here" box. It changes nothing cryptographically and is undoable like any annotation.

```json
{
  "opId": "0193b2e4-8000-7f2e-9a53-6d2f8a1b0c99",
  "ts": "2026-11-12T14:10:00Z",
  "type": "create",
  "target": "annotation",
  "id": "e5f6a7b8-c9d0-4123-8456-789abcdef012",
  "page": 4,
  "payload": {
    "subtype": "Ink",
    "intent": "signature",
    "page": 4,
    "color": "#000000",
    "strokeWidth": 1.8,
    "smoothing": { "algorithm": "catmullRom", "tension": 0.5 },
    "strokes": [ { "points": [[120.0, 140.0], [121.5, 141.2]] } ],
    "signer": {
      "name": "Andrea Rosa",
      "email": "andrea@example.com",
      "reason": "I agree to the terms",
      "location": "Milan, IT",
      "signedAt": "2026-11-12T14:10:00Z",
      "ipHint": null
    },
    "bindToField": "Signature1",
    "fitMode": "contain",
    "flags": { "locked": true, "print": true }
  }
}
```

| Key | Notes |
|---|---|
| `signer` | Stored in `/NibData` and mirrored as `/T` (author) + `/Contents` ("Signed by … on …"). |
| `bindToField` | Fully-qualified name of an **unsigned** `/FT /Sig` field. The engine sets `rect` to the widget's `/Rect` (per `fitMode`: `contain` keeps aspect, `stretch` fills) and, on the field, writes no `/V` — the field stays cryptographically unsigned. |
| `flags.locked` | Recommended `true` so the mark cannot be casually moved. |

### 6.2 Cryptographic digital signature (PAdES)

`target: "signature"`. Only `create` is meaningful (a signature is never updated; `delete` is allowed **only** while the signature is still in `prepared` state, i.e. before `completeSignature` — after that, removing it requires a new revision and invalidates nothing of ours, but is refused by default).

```json
{
  "opId": "0193b2e4-9000-7f2e-9a53-6d2f8a1b0d01",
  "ts": "2026-11-12T14:12:00Z",
  "type": "create",
  "target": "signature",
  "id": "Signature1",
  "page": 4,
  "payload": {
    "fieldName": "Signature1",
    "widget": { "page": 4, "rect": [100, 100, 300, 160] },
    "subFilter": "ETSI.CAdES.detached",
    "digestAlgorithm": "SHA-256",
    "signatureAlgorithm": "RSASSA-PSS",
    "level": "B-B",
    "signer": {
      "commonName": "Andrea Rosa",
      "certificateSha256": "sha256:ab12…",
      "chainSha256": ["sha256:ab12…", "sha256:cd34…"]
    },
    "reason": "Approval",
    "location": "Milan, IT",
    "contactInfo": "andrea@example.com",
    "docMdp": null,
    "lockFieldsAfterSigning": ["applicant.*"],
    "appearance": {
      "mode": "graphicAndDescription",
      "graphicRef": "e5f6a7b8-c9d0-4123-8456-789abcdef012",
      "showName": true, "showDate": true, "showReason": true,
      "fontFamily": "Helvetica", "fontSize": 8
    },
    "timestamp": null,
    "status": "prepared",
    "byteRange": null,
    "contentsLength": 16384,
    "signedAt": null
  }
}
```

| Key | Type | PDF | Notes |
|---|---|---|---|
| `fieldName` | string | `/T` | Creates the field if absent; reuses an existing empty `/Sig` field otherwise. |
| `widget` | `{page, rect}` | `/Rect` | `rect` may be `[0,0,0,0]` for an invisible signature. |
| `subFilter` | `"ETSI.CAdES.detached" \| "adbe.pkcs7.detached"` | `/SubFilter` | Default CAdES (PAdES). `adbe.pkcs7.detached` for legacy validators. |
| `digestAlgorithm` | `"SHA-256" \| "SHA-384" \| "SHA-512"` | CMS | |
| `signatureAlgorithm` | `"RSASSA-PSS" \| "RSA-PKCS1v15" \| "ECDSA-P256" \| "ECDSA-P384"` | CMS | Must match what the Kotlin `Signer` can produce. |
| `level` | `"B-B" \| "B-T"` | CMS attrs | `B-T` requires `timestamp.tsaUrl`; implemented as an extension (see ROADMAP M5 stretch). |
| `signer.*` | hashes | — | Certificates themselves travel in `SignResult`, not in the delta. |
| `reason`, `location`, `contactInfo` | string | `/Reason`, `/Location`, `/ContactInfo` | |
| `docMdp` | `null \| 1 \| 2 \| 3` | `/Reference [<< /TransformMethod /DocMDP /P n >>]` + catalog `/Perms` | Only for the first signature in a document. |
| `lockFieldsAfterSigning` | `string[]` (glob) | `/Lock << /Action /Include /Fields [...] >>` | Field lock dictionary. |
| `appearance.mode` | `"invisible" \| "description" \| "graphic" \| "graphicAndDescription"` | `/AP` (`/FRM`,`/n0`,`/n2`) | `graphicRef` points at an `Ink`/`Stamp` annotation whose content is **copied** into `/n2` (the referenced annotation is then deleted by the engine in the same group). |
| `timestamp` | `null \| { "tsaUrl": "…", "tokenSha256": "…" }` | unsigned attr `signature-time-stamp` | |
| `status` | `"prepared" \| "signed" \| "failed"` | — | Engine-maintained. |
| `byteRange` | `[a,b,c,d] \| null` | `/ByteRange` | Filled by engine after `completeSignature`. |
| `contentsLength` | int | `/Contents` hex length | Placeholder size. |
| `signedAt` | RFC 3339 | `/M` | |

After a signature op with `status: "signed"` exists, the engine rejects any `update`/`delete` whose target existed before that signature unless `OpenOptions.allowIncrementalAnnotations` is set **and** `docMdp` permits it (level 3 or `null`); new `create`s are always allowed (they go into a new incremental revision — standard PDF behaviour).

---

## 7. AcroForm fields

`target: "formField"`, `id` = fully-qualified field name. `create` adds a new field (rare — form *authoring*); the common case is `update` with `{ "value": … }`.

### 7.1 Field entity

```json
{
  "name": "applicant.name",
  "fieldType": "text",
  "value": "Andrea Rosa",
  "defaultValue": "",
  "flags": {
    "readOnly": false, "required": true, "noExport": false,
    "multiline": false, "password": false, "comb": false,
    "doNotSpellCheck": false, "doNotScroll": false, "richText": false
  },
  "maxLen": 64,
  "defaultAppearance": { "fontFamily": "Helvetica", "fontSize": 0, "color": "#000000" },
  "textAlign": "left",
  "tooltip": "Full legal name",
  "widgets": [
    {
      "id": "w-7f3b…",
      "page": 0,
      "rect": [120, 640, 420, 662],
      "appearanceState": null,
      "borderColor": "#000000", "backgroundColor": "#FFFFFF",
      "borderWidth": 1, "borderStyle": { "style": "solid", "dash": null },
      "rotation": 0
    }
  ]
}
```

Common keys:

| Key | PDF | Notes |
|---|---|---|
| `name` | `/T` chain | Fully qualified (`parent.child`). |
| `fieldType` | `/FT` + `/Ff` | `text` (`/Tx`), `checkbox` / `radio` / `pushButton` (`/Btn`), `combo` / `list` (`/Ch`), `signature` (`/Sig`, read-only here; see §6.2). |
| `value` | `/V` | Type depends on `fieldType` (below). |
| `defaultValue` | `/DV` | |
| `flags` | `/Ff` bits | `readOnly`(1) `required`(2) `noExport`(3) + type-specific. |
| `defaultAppearance` | `/DA` | `fontSize: 0` = auto-size. |
| `textAlign` | `/Q` | |
| `tooltip` | `/TU` | |
| `widgets[]` | `/Kids` or merged widget | `appearanceState` → `/AS`; `borderColor`/`backgroundColor` → `/MK /BC` `/BG`. |

### 7.2 Per-type `value` and specifics

| `fieldType` | `value` | Extra keys | Rules |
|---|---|---|---|
| `text` | `string` | `maxLen` (`/MaxLen`), `flags.multiline` (bit 13), `password` (14), `comb` (25) | `MAXLEN_EXCEEDED` if longer than `maxLen`. `/AP` regenerated on each change. |
| `checkbox` | `string` — the **export value** (e.g. `"Yes"`) or `"Off"` | `exportValues: ["Yes"]` (from `/AP /N` keys) | `value` must be `"Off"` or one of `exportValues`. `widgets[].appearanceState` is derived by engine. |
| `radio` | `string` export value of the selected kid or `"Off"` | `exportValues: ["A","B","C"]`, `flags.noToggleToOff` (15), `radiosInUnison` (26) | Each kid widget has one export value; engine sets `/AS` on all kids. |
| `combo` | `string` (selected option export value, or free text if `editable`) | `options: [{ "export": "IT", "label": "Italy" }]` (`/Opt`), `flags.editable` (`/Ff` bit 19 Edit), `sort` (20) | `VALUE_NOT_IN_OPTIONS` unless `editable`. |
| `list` | `string \| string[]` (multi-select when `flags.multiSelect`, bit 22) | `options`, `topIndex` (`/TI`) | Engine sets `/I` indices. |
| `pushButton` | — | `caption` (`/MK /CA`) | No value; included so field enumeration is complete. Actions (`/A`) are read-only metadata. |
| `signature` | — | see §6.2 | Not settable via `formField` ops. |

### 7.3 Typical ops

```json
[
  { "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0d11", "ts": "2026-11-12T14:04:02Z", "type": "update", "target": "formField",
    "id": "applicant.name", "payload": { "value": "Andrea Rosa" } },
  { "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0d12", "ts": "2026-11-12T14:04:02Z", "type": "update", "target": "formField",
    "id": "consent", "payload": { "value": "Yes" } },
  { "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0d13", "ts": "2026-11-12T14:04:02Z", "type": "update", "target": "formField",
    "id": "country", "payload": { "value": "IT" } },
  { "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0d14", "ts": "2026-11-12T14:04:02Z", "type": "update", "target": "formField",
    "id": "plan", "payload": { "value": "Premium" } }
]
```

Field enumeration (`NibDocument.form_fields()`) returns the full entity list above so a host can render its own UI; `calculationOrder` (`/CO`) and JavaScript actions are exposed as read-only metadata and **not** executed (no V8).

---

## 8. Undo / Redo — command pattern over ops

```
          apply(op)                    undo()                      redo()
 UI ────────────────► Engine ─────────────────────► Engine ───────────────────► Engine
                        │  1. validate                │ pop undoStack            │ pop redoStack
                        │  2. snapshot(before)         │ apply(cmd.inverse)       │ apply(cmd.forward)
                        │  3. mutate model             │ push redoStack           │ push undoStack
                        │  4. inverse = derive()       │ emit dirty               │ emit dirty
                        │  5. push Command{fwd,inv}
                        │  6. clear redoStack
                        │  7. emit dirty(bbox∪)
```

```rust
pub struct Command {
    pub forward: Op,            // exactly what was applied (opId preserved)
    pub inverse: Op,            // derived at apply time, new opId, same groupId
    pub dirty: Vec<(u32, Rect)>,
    pub coalesce_key: Option<CoalesceKey>,
}

pub struct UndoManager {
    undo: Vec<Vec<Command>>,    // each entry = one user-visible step (a group)
    redo: Vec<Vec<Command>>,
    history: Vec<Op>,           // append-only log of every forward op *and* every undo/redo inverse, for Full export
    open_group: Option<(Uuid, Instant)>,
    limit: usize,               // default 200 steps; oldest groups dropped (history log is never dropped)
}
```

**Inverse derivation** (pure, from `before` snapshot and `forward`):

| forward | inverse |
|---|---|
| `create E` | `delete E.id` |
| `delete E.id` | `create` with the full snapshot of `E` taken before deletion (incl. `appearance.apHash` so a foreign `/AP` can be restored from the writer's object cache) |
| `update id patch` | `update id patch'` where `patch'` sets every key touched by `patch` to its *previous* value (or `null` if it did not exist). Computed by diffing `before` vs `after` restricted to the key paths in `patch`. Arrays: whole previous array. |

**Grouping & coalescing**

- Ops with the same `groupId` are applied atomically (all validate or none apply) and undo as one step.
- Without an explicit `groupId`, the engine opens an implicit group and coalesces consecutive ops when `coalesce_key` matches and the gap is < 500 ms: `(Update, id, "value")` for text typing; `(Update, id, "rect")` for drags; `(Update, id, "strokes")` for ink additions. Coalesced commands keep the first `forward.opId` and the first inverse snapshot.
- `beginGroup()` / `endGroup()` are exposed for the UI (e.g. "move 5 selected annotations").

**Laws** (property-tested with `proptest`):

1. `apply(op); undo()` ⇒ model ≡ model before `apply` (structural equality ignoring `modifiedAt`).
2. `undo(); redo()` ⇒ model ≡ model after `apply`.
3. For any sequence, `export(Compacted)` applied to a fresh copy of the base document ⇒ same model as the live one.
4. Importing the `Full` export into a fresh engine replays to the same model and the same undo depth.

**Export modes**

| Mode | Content | Use |
|---|---|---|
| `Full` | `history` log in order (forward ops, plus inverse ops emitted by undo/redo, each with their own `opId`) | Audit trail, crash recovery (`SavedStateHandle`), collaboration |
| `Compacted` | Per entity: `create`(final state) if created & not deleted; `update`(diff base→final) if pre-existing & modified; `delete` if pre-existing & deleted; nothing if created & deleted. Signature ops are never compacted away. Ordered by first touch. | Sending changes to a server, writing to PDF, small payload |

**Import semantics**

- Ops with an already-seen `opId` are skipped (idempotent re-import).
- `baseRevision` must equal the current revision, else `REVISION_MISMATCH` unless `ImportOptions.rebase = true`, in which case ops referencing unknown ids are reported and skipped, the rest applied.
- Imported ops are pushed to the undo stack as one group by default (`ImportOptions.undoable`).

---

## 9. Dirty regions & write mapping

Every applied command yields `dirty: [(page, rect)]` in user space; the renderer invalidates intersecting tiles only (see ARCHITECTURE §5.1). The writer (M3) maps ops to lopdf operations on `flush()`:

| Op | lopdf action |
|---|---|
| `create annotation` | new annotation dict + `/AP` XObject(s) (+ image XObject), appended to page `/Annots` (clone page object into incremental doc) |
| `update annotation` | clone annot dict, patch keys, regenerate `/AP` unless `appearance.regenerate == false` |
| `delete annotation` | remove ref from `/Annots`, drop `/Popup`; objects are left orphaned in prior revisions (incremental) |
| `update formField` | clone field (and kids), set `/V`, `/AS`, `/I`; regenerate widget `/AP`; set AcroForm `/NeedAppearances false` |
| `create signature` | field + widget + `/V` sig dict with placeholders; **always** the last change in its revision |

---

## 10. Complete worked example

```json
{
  "schemaVersion": 1,
  "documentId": "sha256:3f1c5a6b7c8d9e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7a8b",
  "pdfId": ["F2A1C3D4E5F60718293A4B5C6D7E8F90", "0A1B2C3D4E5F60718293A4B5C6D7E8F9"],
  "baseRevision": 0,
  "generator": "nib-pdf/0.1.0 (android)",
  "createdAt": "2026-11-12T14:03:22Z",
  "ops": [
    {
      "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c01",
      "ts": "2026-11-12T14:03:25Z",
      "author": "Andrea",
      "type": "create",
      "target": "annotation",
      "id": "c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a55",
      "page": 0,
      "payload": {
        "subtype": "Highlight",
        "page": 0,
        "color": "#FFEB3B",
        "opacity": 0.5,
        "quadPoints": [[100.0, 700.5, 300.2, 700.5, 100.0, 688.0, 300.2, 688.0]]
      }
    },
    {
      "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c02",
      "ts": "2026-11-12T14:03:40Z",
      "type": "update",
      "target": "annotation",
      "id": "c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a55",
      "payload": { "color": "#A5D6A7", "contents": "Agreed" }
    },
    {
      "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c03",
      "ts": "2026-11-12T14:04:02Z",
      "type": "update",
      "target": "formField",
      "id": "applicant.name",
      "payload": { "value": "Andrea Rosa" }
    },
    {
      "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c04",
      "ts": "2026-11-12T14:05:10Z",
      "type": "create",
      "target": "annotation",
      "id": "e5f6a7b8-c9d0-4123-8456-789abcdef012",
      "page": 4,
      "payload": {
        "subtype": "Ink",
        "intent": "signature",
        "page": 4,
        "color": "#000000",
        "strokeWidth": 1.8,
        "smoothing": { "algorithm": "catmullRom", "tension": 0.5, "resampleSpacing": 1.5, "simplifyTolerance": 0.3 },
        "strokes": [
          { "points": [[120.0, 140.0, 0.3, 0], [128.4, 147.9, 0.5, 12], [140.1, 139.2, 0.6, 25]], "tool": "stylus" }
        ],
        "signer": { "name": "Andrea Rosa", "signedAt": "2026-11-12T14:05:10Z" },
        "bindToField": "Signature1",
        "fitMode": "contain",
        "flags": { "locked": true, "print": true }
      }
    },
    {
      "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c05",
      "ts": "2026-11-12T14:06:00Z",
      "type": "create",
      "target": "signature",
      "id": "Signature1",
      "page": 4,
      "payload": {
        "fieldName": "Signature1",
        "widget": { "page": 4, "rect": [100, 100, 300, 160] },
        "subFilter": "ETSI.CAdES.detached",
        "digestAlgorithm": "SHA-256",
        "signatureAlgorithm": "ECDSA-P256",
        "level": "B-B",
        "signer": { "commonName": "Andrea Rosa", "certificateSha256": "sha256:ab12", "chainSha256": ["sha256:ab12"] },
        "reason": "Approval",
        "appearance": { "mode": "graphicAndDescription", "graphicRef": "e5f6a7b8-c9d0-4123-8456-789abcdef012", "showName": true, "showDate": true, "showReason": false },
        "status": "prepared",
        "contentsLength": 16384
      }
    }
  ]
}
```

---

## 11. Versioning & compatibility rules

- Readers **must** accept unknown keys and preserve them (`extra`).
- Readers **must** reject `schemaVersion > supported` with `SCHEMA`.
- New `subtype` / `fieldType` values are additive; unknown values on import produce `SCHEMA` for that op only.
- Numbers are serialized with at most 3 decimals (points) / 4 decimals (opacity, pressure); parsers accept any JSON number.
