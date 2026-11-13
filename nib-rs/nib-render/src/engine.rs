//! The single PDFium engine. `Pdfium` is `!Send + !Sync` without the
//! `thread_safe` feature, so it is created **on** the worker thread and
//! leaked to `&'static` for use by `PdfDocument<'static>` handles.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::thread::{self, ThreadId};

use pdfium_render::prelude::*;

use crate::types::RenderError;

/// Owns the process-wide `Pdfium` instance. Must only be touched from the
/// thread that constructed it; `debug_assert`s enforce that in tests.
pub(crate) struct PdfiumEngine {
    pdfium: &'static Pdfium,
    thread_id: ThreadId,
    // !Sync: Pdfium bindings are only safe on the owning thread.
    _not_sync: PhantomData<*const ()>,
}

impl PdfiumEngine {
    /// Binds and initializes PDFium. Call once, on the render worker thread:
    /// `FPDF_InitLibrary` runs once per process and the engine is leaked.
    pub(crate) fn init_on_current_thread(
        library_dir: Option<&Path>,
    ) -> Result<PdfiumEngine, RenderError> {
        let bindings = match library_dir {
            Some(dir) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                &PathBuf::from(dir),
            )),
            None => Pdfium::bind_to_system_library(),
        }
        .map_err(|e| RenderError::Pdfium(format!("bind: {e}")))?;
        let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
        Ok(PdfiumEngine {
            pdfium,
            thread_id: thread::current().id(),
            _not_sync: PhantomData,
        })
    }

    pub(crate) fn pdfium(&self) -> &'static Pdfium {
        debug_assert_eq!(thread::current().id(), self.thread_id);
        self.pdfium
    }
}
