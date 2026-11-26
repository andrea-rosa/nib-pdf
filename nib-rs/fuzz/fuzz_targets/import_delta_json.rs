#![no_main]

use libfuzzer_sys::fuzz_target;
use nib_core::delta::{Engine, ExportMode, ImportOptions};
use nib_core::model::DeltaDocument;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = serde_json::from_slice::<DeltaDocument>(data) else {
        return;
    };
    for flags in 0..8 {
        let mut engine = Engine::new("fuzz".into(), 4);
        let options = ImportOptions {
            force: flags & 1 != 0,
            rebase: flags & 2 != 0,
            undoable: flags & 4 != 0,
        };
        if engine.import(doc.clone(), options).is_ok() {
            for mode in [ExportMode::Full, ExportMode::Compacted] {
                let _ = engine.export_json(mode, None);
            }
            let _ = engine.undo();
            let _ = engine.redo();
        }
    }
});
