//! Worker-side document store and tile renderer. Runs entirely on the
//! `nib-render` worker thread.

use std::collections::HashMap;
use std::time::Instant;

use nib_core::geom::{Rect, Rotation};
use pdfium_render::prelude::*;

use crate::engine::PdfiumEngine;
use crate::types::{CharBox, PageInfo, RenderError, TileRequest, TileResult};

/// An open PDFium document plus a swap epoch for diagnostics/tests.
struct Doc {
    document: PdfDocument<'static>,
    /// Monotonic swap counter; read by the swap-invariant unit test.
    #[allow(dead_code)]
    epoch: u32,
}

/// One logical document: `current` plus the `previous` revision kept alive
/// while its in-flight tiles drain. Never more than two resident.
#[derive(Default)]
struct DocSlot<D> {
    current: Option<D>,
    previous: Option<D>,
}

impl<D> DocSlot<D> {
    /// Replaces `current`, demoting it to `previous` and dropping the old
    /// `previous` — the ≤2-resident invariant.
    fn swap(&mut self, doc: D) {
        self.previous = self.current.replace(doc);
    }
}

pub(crate) struct Renderer {
    engine: PdfiumEngine,
    docs: HashMap<u64, DocSlot<Doc>>,
    next_doc_id: u64,
    next_epoch: u32,
}

impl Renderer {
    pub fn new(engine: PdfiumEngine) -> Self {
        Renderer {
            engine,
            docs: HashMap::new(),
            next_doc_id: 1,
            next_epoch: 0,
        }
    }

    fn open_doc(&mut self, bytes: Vec<u8>) -> Result<u64, RenderError> {
        let document = self
            .engine
            .pdfium()
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(|e| match e {
                PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                    RenderError::EncryptedUnsupported
                }
                other => RenderError::Parse(other.to_string()),
            })?;
        self.next_epoch += 1;
        let doc = Doc {
            document,
            epoch: self.next_epoch,
        };
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        self.docs.insert(
            id,
            DocSlot {
                current: Some(doc),
                previous: None,
            },
        );
        Ok(id)
    }

    pub fn open(&mut self, bytes: Vec<u8>) -> Result<u64, RenderError> {
        self.open_doc(bytes)
    }

    /// Swaps the bytes backing `id` (new revision after a save). The old
    /// document stays resident as `previous` until the next swap; at most two
    /// documents per id are ever resident. Exists for M3; unit-tested only in M1.
    pub fn swap(&mut self, id: u64, bytes: Vec<u8>) -> Result<(), RenderError> {
        let document = self
            .engine
            .pdfium()
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(|e| match e {
                PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                    RenderError::EncryptedUnsupported
                }
                other => RenderError::Parse(other.to_string()),
            })?;
        self.next_epoch += 1;
        let doc = Doc {
            document,
            epoch: self.next_epoch,
        };
        let slot = self
            .docs
            .get_mut(&id)
            .ok_or_else(|| RenderError::Internal(format!("unknown doc id {id}")))?;
        slot.swap(doc);
        Ok(())
    }

    pub fn close(&mut self, id: u64) {
        self.docs.remove(&id);
    }

    fn page<'a>(&'a self, id: u64, index: u16) -> Result<PdfPage<'a>, RenderError> {
        let slot = self
            .docs
            .get(&id)
            .ok_or_else(|| RenderError::Internal(format!("unknown doc id {id}")))?;
        let doc = slot
            .current
            .as_ref()
            .ok_or_else(|| RenderError::Internal(format!("doc {id} has no document")))?;
        doc.document
            .pages()
            .get(index.into())
            .map_err(|e| RenderError::Pdfium(format!("page {index}: {e}")))
    }

    pub fn page_count(&self, id: u64) -> Result<u32, RenderError> {
        let slot = self
            .docs
            .get(&id)
            .ok_or_else(|| RenderError::Internal(format!("unknown doc id {id}")))?;
        let doc = slot
            .current
            .as_ref()
            .ok_or_else(|| RenderError::Internal(format!("doc {id} has no document")))?;
        Ok(doc.document.pages().len() as u32)
    }

    pub fn page_info(&self, id: u64, index: u16) -> Result<PageInfo, RenderError> {
        let page = self.page(id, index)?;
        let b = page.boundaries();
        let media = b
            .media()
            .map_err(|e| RenderError::Pdfium(e.to_string()))?
            .bounds;
        // Pages without an explicit /CropBox inherit the MediaBox.
        let crop = b.crop().map(|c| c.bounds).unwrap_or(media);
        let rotation = match page
            .rotation()
            .map_err(|e| RenderError::Pdfium(e.to_string()))?
        {
            PdfPageRenderRotation::None => Rotation::R0,
            PdfPageRenderRotation::Degrees90 => Rotation::R90,
            PdfPageRenderRotation::Degrees180 => Rotation::R180,
            PdfPageRenderRotation::Degrees270 => Rotation::R270,
        };
        Ok(PageInfo {
            crop_box: Rect::new(
                crop.left().value as f64,
                crop.bottom().value as f64,
                crop.right().value as f64,
                crop.top().value as f64,
            ),
            media_box: Rect::new(
                media.left().value as f64,
                media.bottom().value as f64,
                media.right().value as f64,
                media.top().value as f64,
            ),
            rotation,
            label: page.label().map(|s| s.to_string()),
            width_pt: page.width().value as f64,
            height_pt: page.height().value as f64,
        })
    }

    pub fn text_in_rect(&self, id: u64, index: u16, rect: Rect) -> Result<String, RenderError> {
        let page = self.page(id, index)?;
        let r = rect.normalize();
        let pdf_rect = PdfRect::new_from_values(
            r.y0 as f32, // bottom
            r.x0 as f32, // left
            r.y1 as f32, // top
            r.x1 as f32, // right
        );
        page.text()
            .map(|t| t.inside_rect(pdf_rect))
            .map_err(|e| RenderError::Pdfium(e.to_string()))
    }

    /// Char bounding boxes whose centre lies inside `rect` (user space).
    /// Uses `loose_bounds` with `tight_bounds` as fallback.
    pub fn char_boxes(&self, id: u64, index: u16, rect: Rect) -> Result<Vec<CharBox>, RenderError> {
        let page = self.page(id, index)?;
        let r = rect.normalize();
        let text = page
            .text()
            .map_err(|e| RenderError::Pdfium(e.to_string()))?;
        let mut out = Vec::new();
        for ch in text.chars().iter() {
            let bounds = ch.loose_bounds().or_else(|_| ch.tight_bounds());
            let Ok(b) = bounds else { continue };
            let cb = Rect::new(
                b.left().value as f64,
                b.bottom().value as f64,
                b.right().value as f64,
                b.top().value as f64,
            );
            let cx = (cb.x0 + cb.x1) / 2.0;
            let cy = (cb.y0 + cb.y1) / 2.0;
            if cx >= r.x0 && cx <= r.x1 && cy >= r.y0 && cy <= r.y1 {
                out.push(CharBox {
                    rect: cb,
                    ch: ch.unicode_char().unwrap_or('\u{FFFD}'),
                });
            }
        }
        Ok(out)
    }

    /// Renders one tile as RGBA (`FPDF_REVERSE_BYTE_ORDER` on a BGRA bitmap),
    /// `stride = width * 4`.
    pub fn render_tile(&self, id: u64, req: &TileRequest) -> Result<TileResult, RenderError> {
        let page = self.page(id, req.key.page)?;
        let w = req.device_rect.width;
        let h = req.device_rect.height;
        let mut bitmap = PdfBitmap::empty(w, h, PdfBitmapFormat::BGRA)
            .map_err(|e| RenderError::Pdfium(e.to_string()))?;
        let config = PdfRenderConfig::new()
            .set_target_size(req.page_device_size.width, req.page_device_size.height)
            .set_origin(-req.device_rect.x, -req.device_rect.y)
            .render_annotations(true)
            .use_lcd_text_rendering(false)
            .set_reverse_byte_order(true)
            .set_clear_color(PdfColor::WHITE);
        let start = Instant::now();
        page.render_into_bitmap_with_config(&mut bitmap, &config)
            .map_err(|e| RenderError::Pdfium(e.to_string()))?;
        let render_ms = start.elapsed().as_millis().min(u16::MAX as u128) as u16;
        Ok(TileResult {
            key: req.key,
            width: w as u32,
            height: h as u32,
            stride: (w * 4) as u32,
            pixels: bitmap.as_raw_bytes(),
            render_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DocSlot;

    /// The swap slot never holds more than two resident revisions.
    #[test]
    fn swap_keeps_at_most_two() {
        let mut slot = DocSlot::default();
        slot.swap(1u32);
        assert_eq!(slot.current, Some(1));
        assert_eq!(slot.previous, None);
        slot.swap(2);
        assert_eq!(slot.current, Some(2));
        assert_eq!(slot.previous, Some(1));
        slot.swap(3);
        // The first previous (1) is gone; only current+previous remain.
        assert_eq!(slot.current, Some(3));
        assert_eq!(slot.previous, Some(2));
    }
}
