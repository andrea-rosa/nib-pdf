//! Golden-image tests: each fixture page is rendered whole at 1 pt = 1 px
//! and compared to a committed PNG by perceptual hash (pHash ≤ 4).
//! Regenerate goldens with:
//!   cargo test -p nib-render regenerate_goldens -- --ignored

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_channel::oneshot;
use image_hasher::HasherConfig;

use crate::testutil::{fixtures_dir, test_worker};
use crate::types::*;

fn golden_dir() -> std::path::PathBuf {
    fixtures_dir().join("golden")
}

/// Waits on a oneshot receiver without an async runtime (tests only).
pub(crate) fn wait_result(
    mut rx: oneshot::Receiver<Result<TileResult, RenderError>>,
    timeout: Duration,
) -> Result<TileResult, RenderError> {
    let start = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(Some(r)) => return r,
            Ok(None) => {
                assert!(
                    start.elapsed() < timeout,
                    "timed out waiting for tile result"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(_canceled) => return Err(RenderError::Cancelled),
        }
    }
}

pub(crate) fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name)).unwrap()
}

static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

pub(crate) fn render_whole_page(name: &str) -> (Arc<crate::RenderWorker>, u64, TileResult) {
    let worker = test_worker();
    let doc = worker.open(fixture_bytes(name)).unwrap();
    let info = worker.page_info(doc, 0).unwrap();
    let w = info.width_pt.round() as i32;
    let h = info.height_pt.round() as i32;
    let k = NEXT_KEY.fetch_add(1, Ordering::Relaxed) as u16;
    let req = TileRequest {
        key: TileKey {
            page: 0,
            bucket: 0,
            tx: 0,
            ty: 0,
            revision: 0,
            flags: k as u8,
        },
        device_rect: IRect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        },
        page_device_size: ISize {
            width: w,
            height: h,
        },
        // u64::MAX so this job is never skipped by bump_generation from
        // cancellation tests sharing the process-wide worker.
        generation: u64::MAX,
        priority: Priority::Visible,
        distance: 0,
    };
    let rx = worker.render_tile(doc, req);
    let tile = wait_result(rx, Duration::from_secs(30)).unwrap();
    (worker, doc, tile)
}

fn phash_dist(name: &str) -> u32 {
    let (worker, doc, tile) = render_whole_page(name);
    worker.close(doc);
    let rendered = image::RgbaImage::from_raw(tile.width, tile.height, tile.pixels).unwrap();
    let golden = image::open(golden_dir().join(format!("{}.png", name.trim_end_matches(".pdf"))))
        .unwrap()
        .to_rgba8();
    let hasher = HasherConfig::new().to_hasher();
    let d = hasher
        .hash_image(&rendered)
        .dist(&hasher.hash_image(&golden));
    eprintln!("{name}: pHash distance {d}");
    d
}

macro_rules! golden_test {
    ($test:ident, $fixture:literal) => {
        #[test]
        fn $test() {
            let d = phash_dist($fixture);
            assert!(d <= 4, "{}: pHash distance {d} > 4", $fixture);
        }
    };
}

golden_test!(golden_text, "text.pdf");
golden_test!(golden_vector, "vector.pdf");
golden_test!(golden_image, "image.pdf");
golden_test!(golden_rotated90, "rotated90.pdf");
golden_test!(golden_cropbox, "cropbox.pdf");

#[test]
#[ignore]
fn regenerate_goldens() {
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for fixture in ["text", "vector", "image", "rotated90", "cropbox"] {
        let name = format!("{fixture}.pdf");
        let (worker, doc, tile) = render_whole_page(&name);
        image::save_buffer(
            dir.join(format!("{fixture}.png")),
            &tile.pixels,
            tile.width,
            tile.height,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        worker.close(doc);
    }
}
