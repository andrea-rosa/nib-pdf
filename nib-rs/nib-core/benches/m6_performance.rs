use chrono::{Duration, TimeZone, Utc};
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use nib_core::ap::{self, NoAssets, NoFonts};
use nib_core::delta::{Engine, ExportMode};
use nib_core::geom::{PageTransform, Point, Rect, Rotation};
use nib_core::model::annotation::{AnnotBase, InkAnnot, Stroke};
use nib_core::model::{Annotation, Op, OpKind, Target};
use serde_json::json;
use std::hint::black_box;
use uuid::Uuid;

const UUID_BASE: u128 = 0x9b3a_6c1e_0000_4000_8000_0000_0000_0000;

fn geometry(c: &mut Criterion) {
    let points: Vec<_> = (0..100)
        .map(|i| Point::new(i as f64, (i * 37 % 101) as f64))
        .collect();
    let rect = Rect::new(0.0, 0.0, 612.0, 792.0);

    c.bench_function("geometry/page_transform_roundtrip_100", |b| {
        b.iter(|| {
            let transform = PageTransform::new(rect, Rotation::R90, 2.0);
            black_box(
                points
                    .iter()
                    .copied()
                    .map(|point| transform.to_user(transform.to_device(point)))
                    .collect::<Vec<_>>(),
            )
        })
    });
}

fn annotation() -> Annotation {
    Annotation::Ink(InkAnnot {
        base: AnnotBase {
            id: Uuid::nil(),
            page: 0,
            rect: None,
            color: None,
            opacity: 1.0,
            author: None,
            contents: None,
            subject: None,
            created_at: None,
            modified_at: None,
            flags: Default::default(),
            in_reply_to: None,
            popup: None,
            appearance: Default::default(),
            custom: Default::default(),
            extra: Default::default(),
        },
        stroke_width: Some(2.0),
        line_cap: None,
        line_join: None,
        pressure_sensitive: None,
        min_width_factor: None,
        max_width_factor: None,
        smoothing: None,
        strokes: vec![Stroke {
            points: (0..128)
                .map(|i| vec![i as f64, (i * 37 % 101) as f64])
                .collect(),
            tool: None,
            started_at: None,
        }],
        intent: None,
        signer: None,
        bind_to_field: None,
        bind_to_widget: None,
        fit_mode: None,
    })
}

fn appearance_generation(c: &mut Criterion) {
    let annotation = annotation();

    c.bench_function("ap/ink_128_points", |b| {
        b.iter(|| {
            black_box(ap::build(black_box(&annotation), &NoAssets, &NoFonts).unwrap());
        })
    });
}

fn create_ops(count: usize) -> Vec<Op> {
    let timestamp = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    (0..count)
        .map(|index| {
            let id = Uuid::from_u128(UUID_BASE + 100_000 + index as u128);
            Op {
                op_id: Uuid::from_u128(UUID_BASE + index as u128),
                ts: timestamp + Duration::milliseconds(index as i64),
                author: None,
                group_id: None,
                kind: OpKind::Create,
                target: Target::Annotation,
                id: id.to_string(),
                page: Some(0),
                payload: json!({
                    "subtype": "Square",
                    "page": 0,
                    "rect": [10.0, 10.0, 50.0, 50.0],
                    "color": "#FF0000"
                }),
            }
        })
        .collect()
}

fn populated_engine() -> Engine {
    let mut engine = Engine::new("sha256:criterion".into(), 1);
    engine.apply(create_ops(50)).unwrap();
    engine
}

fn delta_apply_and_compaction(c: &mut Criterion) {
    c.bench_function("delta/apply_50", |b| {
        b.iter_batched(
            || (Engine::new("sha256:criterion".into(), 1), create_ops(50)),
            |(mut engine, ops)| {
                black_box(engine.apply(ops).unwrap());
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("delta/compact_50", |b| {
        b.iter_batched(
            populated_engine,
            |engine| black_box(engine.export(ExportMode::Compacted, None)),
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    m6_performance,
    geometry,
    appearance_generation,
    delta_apply_and_compaction
);
criterion_main!(m6_performance);
