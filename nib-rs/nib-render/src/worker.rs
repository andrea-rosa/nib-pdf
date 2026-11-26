//! `RenderWorker`: one OS thread named `nib-render` owning the `PdfiumEngine`
//! and the `Renderer`. Callers enqueue control messages (mpsc) and tile jobs
//! (bounded priority heap) and receive results on mpsc/oneshot channels.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use futures_channel::oneshot;
use nib_core::geom::Rect;

use crate::engine::PdfiumEngine;
use crate::queue::State;
use crate::renderer::Renderer;
use crate::types::{CharBox, FieldValue, PageInfo, RenderError, TileRequest, TileResult};

enum Ctrl {
    Open {
        bytes: Vec<u8>,
        reply: mpsc::Sender<Result<u64, RenderError>>,
    },
    Close {
        doc: u64,
    },
    PageCount {
        doc: u64,
        reply: mpsc::Sender<Result<u32, RenderError>>,
    },
    PageInfo {
        doc: u64,
        index: u16,
        reply: mpsc::Sender<Result<PageInfo, RenderError>>,
    },
    PageInfos {
        doc: u64,
        reply: mpsc::Sender<Result<Vec<PageInfo>, RenderError>>,
    },
    TextInRect {
        doc: u64,
        index: u16,
        rect: Rect,
        reply: mpsc::Sender<Result<String, RenderError>>,
    },
    CharBoxes {
        doc: u64,
        index: u16,
        rect: Rect,
        reply: mpsc::Sender<Result<Vec<CharBox>, RenderError>>,
    },
    FormFieldValues {
        doc: u64,
        reply: mpsc::Sender<Result<Vec<FieldValue>, RenderError>>,
    },
    Swap {
        doc: u64,
        bytes: Vec<u8>,
        reply: mpsc::Sender<Result<(), RenderError>>,
    },
    Shutdown,
    /// Parks the worker until a message arrives on `gate`; test-only hook to
    /// deterministically fill the queue before releasing it.
    #[cfg(test)]
    Block(mpsc::Receiver<()>),
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

/// Handle to the render worker thread.
pub struct RenderWorker {
    shared: Arc<Shared>,
    ctrl: mpsc::Sender<Ctrl>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl RenderWorker {
    /// Spawns the worker thread. `library_dir` locates the pdfium binary on
    /// host builds; `None` binds the already-mapped system library (Android).
    pub fn spawn(library_dir: Option<PathBuf>) -> Arc<RenderWorker> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::new()),
            cv: Condvar::new(),
        });
        let (ctrl_tx, ctrl_rx) = mpsc::channel::<Ctrl>();
        let worker_shared = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("nib-render".into())
            .spawn(move || worker_main(worker_shared, ctrl_rx, library_dir))
            .expect("spawn nib-render thread");
        Arc::new(RenderWorker {
            shared,
            ctrl: ctrl_tx,
            handle: Mutex::new(Some(handle)),
        })
    }

    /// Opens a document on the worker thread; returns its handle id.
    pub fn open(&self, bytes: Vec<u8>) -> Result<u64, RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::Open { bytes, reply: tx })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    /// Swaps the bytes of an open document (new revision). Keeps ≤ 2 resident.
    pub fn swap(&self, doc: u64, bytes: Vec<u8>) -> Result<(), RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::Swap {
            doc,
            bytes,
            reply: tx,
        })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    /// Closes a document handle.
    pub fn close(&self, doc: u64) {
        let _ = self.ctrl.send(Ctrl::Close { doc });
        self.wake();
    }

    pub fn page_count(&self, doc: u64) -> Result<u32, RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::PageCount { doc, reply: tx })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    pub fn page_info(&self, doc: u64, index: u16) -> Result<PageInfo, RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::PageInfo {
            doc,
            index,
            reply: tx,
        })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    /// Loads all page metadata in document order with one worker control message.
    pub fn page_infos(&self, doc: u64) -> Result<Vec<PageInfo>, RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::PageInfos { doc, reply: tx })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    pub fn text_in_rect(&self, doc: u64, index: u16, rect: Rect) -> Result<String, RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::TextInRect {
            doc,
            index,
            rect,
            reply: tx,
        })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    /// Char bounding boxes inside `rect` on page `index` (user space).
    pub fn char_boxes(
        &self,
        doc: u64,
        index: u16,
        rect: Rect,
    ) -> Result<Vec<CharBox>, RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::CharBoxes {
            doc,
            index,
            rect,
            reply: tx,
        })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    /// Form field values as pdfium sees them on the current revision.
    pub fn form_field_values(&self, doc: u64) -> Result<Vec<FieldValue>, RenderError> {
        let (tx, rx) = mpsc::channel();
        self.send_ctrl(Ctrl::FormFieldValues { doc, reply: tx })?;
        rx.recv()
            .unwrap_or_else(|_| Err(RenderError::Internal("worker died".into())))
    }

    /// Enqueues a tile job; the returned receiver resolves with the result.
    /// Dropping the receiver before the job starts marks it cancelled and the
    /// worker skips it.
    pub fn render_tile(
        &self,
        doc: u64,
        req: TileRequest,
    ) -> oneshot::Receiver<Result<TileResult, RenderError>> {
        let (tx, rx) = oneshot::channel();
        let evicted = {
            let mut state = self.shared.state.lock().unwrap();
            if state.closed {
                drop(state);
                let _ = tx.send(Err(RenderError::Cancelled));
                return rx;
            }
            state.push(doc, req, tx)
        };
        // Reply to the evicted/rejected job only after releasing the lock —
        // `send` runs the receiver's waker inline and UniFFI resumes Kotlin
        // continuations on this thread, which may call back into the worker.
        if let Some(reply) = evicted {
            let _ = reply.send(Err(RenderError::Cancelled));
        }
        self.shared.cv.notify_one();
        rx
    }

    /// Bumps the scheduling generation; older queued jobs are skipped.
    /// Returns the new generation so callers can tag subsequent requests.
    /// Synchronous under the shared lock: the bump happens-before any job
    /// the caller enqueues afterwards, and there is no worker round-trip
    /// (which could deadlock when the worker is parked in `Block`).
    pub fn bump_generation(&self) -> Result<u64, RenderError> {
        let mut state = self.shared.state.lock().unwrap();
        state.generation += 1;
        Ok(state.generation)
    }

    /// Shuts the worker down and joins the thread.
    pub fn shutdown(&self) {
        let _ = self.send_ctrl(Ctrl::Shutdown);
        {
            let mut state = self.shared.state.lock().unwrap();
            state.closed = true;
        }
        self.shared.cv.notify_all();
        if let Some(h) = self.handle.lock().unwrap().take() {
            let _ = h.join();
        }
    }

    #[cfg(test)]
    pub(crate) fn block(&self, gate: mpsc::Receiver<()>) {
        let _ = self.send_ctrl(Ctrl::Block(gate));
    }

    fn send_ctrl(&self, msg: Ctrl) -> Result<(), RenderError> {
        self.ctrl
            .send(msg)
            .map_err(|_| RenderError::Internal("worker died".into()))?;
        self.wake();
        Ok(())
    }

    fn wake(&self) {
        // Flag is inside the same lock as the heap, so the worker can never
        // miss a pending control message while it waits on the condvar.
        self.shared.state.lock().unwrap().ctrl_pending = true;
        self.shared.cv.notify_one();
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_main(shared: Arc<Shared>, ctrl_rx: mpsc::Receiver<Ctrl>, library_dir: Option<PathBuf>) {
    let engine = match PdfiumEngine::init_on_current_thread(library_dir.as_deref()) {
        Ok(e) => e,
        Err(e) => {
            log::error!("nib-render: pdfium init failed: {e}");
            return;
        }
    };
    let mut renderer = Renderer::new(engine);

    let mut state = shared.state.lock().unwrap();
    loop {
        if state.ctrl_pending {
            state.ctrl_pending = false;
            drop(state);
            if handle_ctrls(&shared, &ctrl_rx, &mut renderer) {
                return;
            }
            state = shared.state.lock().unwrap();
            continue;
        }
        if state.closed {
            return;
        }
        match state.heap.pop() {
            Some(job) => {
                // Skip stale or already-cancelled jobs without touching pdfium.
                if job.req.generation < state.generation || job.reply.is_canceled() {
                    continue;
                }
                drop(state);
                let result = renderer.render_tile(job.doc, &job.req);
                let _ = job.reply.send(result);
                state = shared.state.lock().unwrap();
            }
            None => {
                state = shared.cv.wait(state).unwrap();
            }
        }
    }
}

/// Drains the control channel. Returns `true` on `Shutdown`.
fn handle_ctrls(
    shared: &Arc<Shared>,
    ctrl_rx: &mpsc::Receiver<Ctrl>,
    renderer: &mut Renderer,
) -> bool {
    while let Ok(msg) = ctrl_rx.try_recv() {
        match msg {
            Ctrl::Open { bytes, reply } => {
                let result = catch_unwind(AssertUnwindSafe(|| renderer.open(bytes)))
                    .unwrap_or_else(|_| Err(RenderError::Internal("panic in PDFium open".into())));
                let _ = reply.send(result);
            }
            Ctrl::Close { doc } => renderer.close(doc),
            Ctrl::PageCount { doc, reply } => {
                let _ = reply.send(renderer.page_count(doc));
            }
            Ctrl::PageInfo { doc, index, reply } => {
                let _ = reply.send(renderer.page_info(doc, index));
            }
            Ctrl::PageInfos { doc, reply } => {
                let infos = renderer.page_count(doc).and_then(|count| {
                    (0..count)
                        .map(|index| renderer.page_info(doc, index as u16))
                        .collect()
                });
                let _ = reply.send(infos);
            }
            Ctrl::TextInRect {
                doc,
                index,
                rect,
                reply,
            } => {
                let _ = reply.send(renderer.text_in_rect(doc, index, rect));
            }
            Ctrl::CharBoxes {
                doc,
                index,
                rect,
                reply,
            } => {
                let _ = reply.send(renderer.char_boxes(doc, index, rect));
            }
            Ctrl::FormFieldValues { doc, reply } => {
                let _ = reply.send(renderer.form_field_values(doc));
            }
            Ctrl::Swap { doc, bytes, reply } => {
                let result = catch_unwind(AssertUnwindSafe(|| renderer.swap(doc, bytes)))
                    .unwrap_or_else(|_| Err(RenderError::Internal("panic in PDFium swap".into())));
                let _ = reply.send(result);
            }
            Ctrl::Shutdown => {
                shared.state.lock().unwrap().closed = true;
                return true;
            }
            #[cfg(test)]
            Ctrl::Block(gate) => {
                // Park until the test releases us.
                let _ = gate.recv();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::golden::{fixture_bytes, render_whole_page, wait_result};
    use crate::testutil::test_worker;
    use crate::types::*;
    use crate::RenderWorker;

    fn tile_req(generation: u64, priority: Priority) -> TileRequest {
        TileRequest {
            key: TileKey {
                page: 0,
                bucket: 0,
                tx: 0,
                ty: 0,
                revision: 0,
                flags: 0,
            },
            device_rect: IRect {
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            },
            page_device_size: ISize {
                width: 200,
                height: 200,
            },
            generation,
            priority,
            distance: 0,
        }
    }

    fn open_text(worker: &RenderWorker) -> u64 {
        worker.open(fixture_bytes("text.pdf")).unwrap()
    }

    /// Queue up gen-0 jobs behind a blocked worker, bump the generation, then
    /// release: almost all stale jobs are skipped (≤ 5 races through).
    #[test]
    fn cancel_stale_generation() {
        let worker = test_worker();
        let doc = open_text(&worker);
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        worker.block(gate_rx);
        // Wait until the worker is parked inside Block: the Block ctrl was
        // queued first, so enqueue after a short settle, then verify queue
        // grows only after parking (jobs pile up regardless).
        std::thread::sleep(Duration::from_millis(50));

        let mut rxs = Vec::new();
        for _ in 0..200 {
            rxs.push(worker.render_tile(doc, tile_req(0, Priority::Prefetch)));
        }
        worker.bump_generation().unwrap();
        gate_tx.send(()).unwrap();

        let mut ok = 0;
        for rx in rxs {
            if wait_result(rx, Duration::from_secs(30)).is_ok() {
                ok += 1;
            }
        }
        assert!(ok <= 5, "{ok} stale-generation jobs rendered");
        worker.close(doc);
    }

    /// Each bump returns the new, monotonically increasing generation so
    /// callers (e.g. a fresh scheduler after a config change) can tag their
    /// requests with the worker's actual generation instead of a local guess.
    #[test]
    fn bump_generation_returns_monotonic() {
        let worker = test_worker();
        let first = worker.bump_generation().unwrap();
        let second = worker.bump_generation().unwrap();
        assert_eq!(second, first + 1);
    }

    /// A job whose reply receiver is dropped is skipped without rendering.
    #[test]
    fn dropped_receiver_skips_render() {
        let worker = test_worker();
        let doc = open_text(&worker);
        {
            let _rx = worker.render_tile(doc, tile_req(u64::MAX, Priority::Visible));
        } // receiver dropped immediately
        std::thread::sleep(Duration::from_millis(100));
        // Worker is still alive and renders a real tile.
        let rx = worker.render_tile(doc, tile_req(u64::MAX, Priority::Visible));
        let tile = wait_result(rx, Duration::from_secs(30)).unwrap();
        assert_eq!(tile.stride, tile.width * 4);
        worker.close(doc);
    }

    /// page_info reports /Rotate and the post-rotation display size, and the
    /// crop-box offset for a cropped page.
    #[test]
    fn page_info_rotation() {
        let worker = test_worker();
        let doc = worker.open(fixture_bytes("rotated90.pdf")).unwrap();
        let info = worker.page_info(doc, 0).unwrap();
        assert_eq!(info.rotation, nib_core::geom::Rotation::R90);
        // Media box 200×300, /Rotate 90 → display 300×200.
        assert_eq!((info.width_pt, info.height_pt), (300.0, 200.0));
        worker.close(doc);

        let doc = worker.open(fixture_bytes("cropbox.pdf")).unwrap();
        let info = worker.page_info(doc, 0).unwrap();
        assert_eq!(info.crop_box.x0, 100.0);
        assert_eq!(info.crop_box.y0, 150.0);
        assert_eq!(info.crop_box.x1, 500.0);
        assert_eq!(info.crop_box.y1, 750.0);
        worker.close(doc);
    }

    #[test]
    fn page_infos_batch_preserves_page_metadata() {
        let worker = test_worker();
        let doc = worker.open(fixture_bytes("rotated90.pdf")).unwrap();
        let infos = worker.page_infos(doc).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].rotation, nib_core::geom::Rotation::R90);
        assert_eq!((infos[0].width_pt, infos[0].height_pt), (300.0, 200.0));
        worker.close(doc);

        let doc = worker.open(fixture_bytes("cropbox.pdf")).unwrap();
        let infos = worker.page_infos(doc).unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].crop_box.x0, 100.0);
        assert_eq!(infos[0].crop_box.y0, 150.0);
        assert_eq!(infos[0].crop_box.x1, 500.0);
        assert_eq!(infos[0].crop_box.y1, 750.0);
        worker.close(doc);

        let doc = worker.open(fixture_bytes("text-100.pdf")).unwrap();
        let infos = worker.page_infos(doc).unwrap();
        assert_eq!(infos.len(), 100);
        assert_eq!(
            infos[0].width_pt,
            worker.page_info(doc, 0).unwrap().width_pt
        );
        assert_eq!(
            infos[99].height_pt,
            worker.page_info(doc, 99).unwrap().height_pt
        );
        worker.close(doc);
    }

    /// `text.pdf` has 4 Helvetica lines at a 16 pt pitch; snapped lines keep
    /// that pitch and share one height.
    #[test]
    fn char_boxes_text_fixture_four_lines() {
        let worker = test_worker();
        let doc = open_text(&worker);
        let chars = worker
            .char_boxes(doc, 0, nib_core::geom::Rect::new(30.0, 105.0, 190.0, 170.0))
            .unwrap();
        assert!(!chars.is_empty());
        let lines = nib_core::text::snap_lines(
            &chars,
            nib_core::geom::Rect::new(30.0, 105.0, 190.0, 170.0),
        );
        assert_eq!(lines.len(), 4, "lines: {lines:?}");
        for w in lines.windows(2) {
            let pitch = w[0].y1 - w[1].y1;
            assert!((pitch - 16.0).abs() <= 0.75, "pitch {pitch}");
            assert!((w[0].height() - w[1].height()).abs() <= 0.5);
        }
        worker.close(doc);
    }

    /// Smoke: a real tile renders with RGBA stride and non-zero timing data.
    #[test]
    fn render_tile_smoke() {
        let (worker, doc, tile) = render_whole_page("text.pdf");
        assert!(tile.width > 0 && tile.height > 0);
        assert_eq!(tile.stride, tile.width * 4);
        assert_eq!(tile.pixels.len() as u32, tile.stride * tile.height);
        worker.close(doc);
    }
}
