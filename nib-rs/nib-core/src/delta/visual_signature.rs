use crate::geom::Rect;
use crate::model::{
    Annotation, FieldType, FitMode, InkAnnot, Intent, SigStatus, SignatureFieldState,
    SmoothingAlgorithm, StampKind, VisualSigner,
};

use super::{err, valid_rect, ErrorCode, Model, ValidationError};

pub(super) fn prepare(
    annotation: &mut Annotation,
    model: &Model,
    op_index: usize,
) -> Result<(), Vec<ValidationError>> {
    let (intent, signer, field_name, widget_id, fit) = match annotation {
        Annotation::Ink(ink) => (
            ink.intent,
            ink.signer.clone(),
            ink.bind_to_field.clone(),
            ink.bind_to_widget.clone(),
            ink.fit_mode.unwrap_or(FitMode::Contain),
        ),
        Annotation::Stamp(stamp) => (
            stamp.intent,
            stamp.signer.clone(),
            stamp.bind_to_field.clone(),
            stamp.bind_to_widget.clone(),
            stamp.fit_mode.unwrap_or(FitMode::Contain),
        ),
        _ => return Ok(()),
    };

    if intent.is_some() {
        set_signer_metadata(annotation, signer.as_ref());
    }
    let Some(field_name) = field_name else {
        if widget_id.is_some() {
            return Err(err(
                op_index,
                "payload.bindToWidget",
                ErrorCode::Schema,
                "bindToWidget requires bindToField",
            ));
        }
        return Ok(());
    };
    if !matches!(intent, Some(Intent::Signature | Intent::Initials)) {
        return Err(err(
            op_index,
            "payload.intent",
            ErrorCode::Schema,
            "binding requires signature or initials intent",
        ));
    }
    let Some(field) = model.form_field(&field_name) else {
        return Err(err(
            op_index,
            "payload.bindToField",
            ErrorCode::UnknownId,
            format!("unknown signature field {field_name}"),
        ));
    };
    if field.field_type != FieldType::Signature {
        return Err(err(
            op_index,
            "payload.bindToField",
            ErrorCode::Schema,
            "field is not a signature field",
        ));
    }
    if field.flags.read_only {
        return Err(err(
            op_index,
            "payload.bindToField",
            ErrorCode::FieldReadonly,
            "signature field is read-only",
        ));
    }
    if field.signature_state != Some(SignatureFieldState::Empty)
        || model.signatures.values().any(|signature| {
            signature.field_name == field_name && signature.status != SigStatus::Failed
        })
    {
        return Err(err(
            op_index,
            "payload.bindToField",
            ErrorCode::SignatureLocked,
            "signature field is occupied or its state is unknown",
        ));
    }

    let mut widgets = field.widgets.iter().filter(|widget| {
        widget.page == annotation.base().page
            && widget_id
                .as_ref()
                .is_none_or(|id| widget.id.as_ref() == Some(id))
    });
    let Some(widget) = widgets.next() else {
        return Err(err(
            op_index,
            "payload.bindToWidget",
            ErrorCode::UnknownId,
            "signature field has no matching widget on this page",
        ));
    };
    if widgets.next().is_some() {
        return Err(err(
            op_index,
            "payload.bindToWidget",
            ErrorCode::Schema,
            "multiple signature widgets match; specify bindToWidget",
        ));
    }
    let target = widget.rect.normalize();
    if !renderable_rect(&target) {
        return Err(err(
            op_index,
            "payload.bindToWidget",
            ErrorCode::RectInvalid,
            "signature widget rectangle cannot be represented in the PDF appearance",
        ));
    }
    match annotation {
        Annotation::Ink(ink) => fit_ink(ink, target, fit, op_index)?,
        Annotation::Stamp(stamp) => {
            match stamp.kind.unwrap_or(StampKind::Standard) {
                StampKind::Image => {
                    let usable = stamp.image.as_ref().is_some_and(|image| {
                        (image.asset_id.as_ref().is_some_and(|id| !id.is_empty())
                            || image
                                .data_base64
                                .as_ref()
                                .is_some_and(|data| !data.is_empty()))
                            && image.width_px != Some(0)
                            && image.height_px != Some(0)
                    });
                    if !usable {
                        return Err(err(
                            op_index,
                            "payload.image",
                            ErrorCode::Schema,
                            "signature image needs usable data or an asset and nonzero dimensions",
                        ));
                    }
                }
                StampKind::Vector => {
                    let usable = stamp.vector.as_ref().is_some_and(|vector| {
                        vector.view_box.iter().all(|value| value.is_finite())
                            && vector.view_box[2] > 0.0
                            && vector.view_box[3] > 0.0
                            && !vector.paths.is_empty()
                    });
                    if !usable {
                        return Err(err(
                            op_index,
                            "payload.vector",
                            ErrorCode::RectInvalid,
                            "signature vector needs a finite, nondegenerate viewBox and paths",
                        ));
                    }
                }
                StampKind::Standard => {}
            }
            stamp.preserve_aspect = fit == FitMode::Contain;
            stamp.base.rect = Some(target);
        }
        _ => unreachable!(),
    }
    annotation.base_mut().flags.locked = true;
    annotation.base_mut().flags.print = true;
    Ok(())
}

fn renderable_rect(rect: &Rect) -> bool {
    let [x0, y0, x1, y1] = [
        rect.x0 as f32,
        rect.y0 as f32,
        rect.x1 as f32,
        rect.y1 as f32,
    ];
    valid_rect(rect)
        && rect.width() >= 0.001
        && rect.height() >= 0.001
        && (rect.width() as f32).is_finite()
        && (rect.height() as f32).is_finite()
        && [x0, y0, x1, y1].iter().all(|value| value.is_finite())
        && x0 < x1
        && y0 < y1
}

fn renderable_stroke(width: f64, min_factor: f64) -> bool {
    width.is_finite()
        && (width as f32).is_finite()
        && width >= 0.001
        && (width * min_factor).is_finite()
        && width * min_factor >= 0.001
}

fn set_signer_metadata(annotation: &mut Annotation, signer: Option<&VisualSigner>) {
    let Some(signer) = signer else { return };
    let base = annotation.base_mut();
    if base.author.is_none() {
        base.author = signer.name.clone();
    }
    if base.contents.is_none() {
        base.contents = match (&signer.name, &signer.signed_at) {
            (Some(name), Some(date)) => Some(format!("Signed by {name} on {}", date.to_rfc3339())),
            (Some(name), None) => Some(format!("Signed by {name}")),
            (None, Some(date)) => Some(format!("Signed on {}", date.to_rfc3339())),
            (None, None) => None,
        };
    }
}

fn fit_ink(
    ink: &mut InkAnnot,
    target: Rect,
    fit: FitMode,
    op_index: usize,
) -> Result<(), Vec<ValidationError>> {
    let width = ink.stroke_width.unwrap_or(2.0);
    let min_factor = ink.min_width_factor.unwrap_or(0.4);
    let factor = if ink.pressure_sensitive == Some(true) {
        ink.max_width_factor.unwrap_or(1.6)
    } else {
        1.0
    };
    if !width.is_finite()
        || width <= 0.0
        || !factor.is_finite()
        || factor <= 0.0
        || (ink.pressure_sensitive == Some(true)
            && (!min_factor.is_finite() || min_factor <= 0.0 || min_factor > factor))
    {
        return Err(err(
            op_index,
            "payload.strokeWidth",
            ErrorCode::Schema,
            "signature stroke width must be positive and finite",
        ));
    }
    let mut bounds: Option<Rect> = None;
    for point in ink.strokes.iter().flat_map(|stroke| &stroke.points) {
        if point.len() < 2 || !point[0].is_finite() || !point[1].is_finite() {
            return Err(err(
                op_index,
                "payload.strokes",
                ErrorCode::Schema,
                "signature points must have finite coordinates",
            ));
        }
        let point_rect = Rect::new(point[0], point[1], point[0], point[1]);
        bounds = Some(bounds.map_or(point_rect, |rect| rect.union(&point_rect)));
    }
    if ink.pressure_sensitive != Some(true)
        && matches!(
            ink.smoothing.as_ref().map(|s| s.algorithm),
            Some(SmoothingAlgorithm::CatmullRom)
        )
    {
        let tension = ink
            .smoothing
            .as_ref()
            .and_then(|s| s.tension)
            .unwrap_or(0.5);
        if !tension.is_finite() || !(0.0..=1.0).contains(&tension) {
            return Err(err(
                op_index,
                "payload.smoothing.tension",
                ErrorCode::Schema,
                "signature smoothing tension must be between 0 and 1",
            ));
        }
        let t = tension / 3.0;
        for stroke in &ink.strokes {
            let points = &stroke.points;
            if points.len() < 3 {
                continue;
            }
            for index in 0..points.len() - 1 {
                let p0 = &points[index.saturating_sub(1)];
                let p1 = &points[index];
                let p2 = &points[index + 1];
                let p3 = &points[(index + 2).min(points.len() - 1)];
                for (x, y) in [
                    (p1[0] + (p2[0] - p0[0]) * t, p1[1] + (p2[1] - p0[1]) * t),
                    (p2[0] - (p3[0] - p1[0]) * t, p2[1] - (p3[1] - p1[1]) * t),
                ] {
                    let point = Rect::new(x, y, x, y);
                    bounds = Some(bounds.map_or(point, |rect| rect.union(&point)));
                }
            }
        }
    }
    let Some(bounds) = bounds else {
        return Err(err(
            op_index,
            "payload.strokes",
            ErrorCode::Schema,
            "signature must contain at least one point",
        ));
    };
    let source = super::dirty::inflate(&bounds, width * factor / 2.0);
    if !valid_rect(&source) || !source.width().is_finite() || !source.height().is_finite() {
        return Err(err(
            op_index,
            "payload.strokes",
            ErrorCode::RectInvalid,
            "signature geometry is invalid",
        ));
    }
    if let Some(rect) = ink.base.rect {
        let coordinate_tolerance = 0.001;
        let stroke_tolerance = coordinate_tolerance + 0.00025 * (width + factor);
        let same_rect = (rect.x0 - target.x0).abs() <= coordinate_tolerance
            && (rect.y0 - target.y0).abs() <= coordinate_tolerance
            && (rect.x1 - target.x1).abs() <= coordinate_tolerance
            && (rect.y1 - target.y1).abs() <= coordinate_tolerance;
        let contained = source.x0 >= target.x0 - stroke_tolerance
            && source.y0 >= target.y0 - stroke_tolerance
            && source.x1 <= target.x1 + stroke_tolerance
            && source.y1 <= target.y1 + stroke_tolerance;
        let fitted_x = (source.x0 - target.x0).abs() <= stroke_tolerance
            && (source.x1 - target.x1).abs() <= stroke_tolerance;
        let fitted_y = (source.y0 - target.y0).abs() <= stroke_tolerance
            && (source.y1 - target.y1).abs() <= stroke_tolerance;
        if same_rect && contained && (fitted_x || fitted_y) {
            if !renderable_stroke(
                width,
                if ink.pressure_sensitive == Some(true) {
                    min_factor
                } else {
                    1.0
                },
            ) {
                return Err(err(
                    op_index,
                    "payload.strokeWidth",
                    ErrorCode::RectInvalid,
                    "fitted signature stroke is too small or large to render",
                ));
            }
            ink.base.rect = Some(target);
            return Ok(());
        }
    }
    let sx = target.width() / source.width();
    let sy = target.height() / source.height();
    let (sx, sy) = if fit == FitMode::Contain {
        let scale = sx.min(sy);
        (scale, scale)
    } else {
        (sx, sy)
    };
    let scaled_width = width * sx.min(sy);
    if !sx.is_finite()
        || !sy.is_finite()
        || sx <= 0.0
        || sy <= 0.0
        || !renderable_stroke(
            scaled_width,
            if ink.pressure_sensitive == Some(true) {
                min_factor
            } else {
                1.0
            },
        )
    {
        return Err(err(
            op_index,
            "payload.strokeWidth",
            ErrorCode::RectInvalid,
            "fitted signature geometry is too small or large to render",
        ));
    }
    let x = target.x0 + (target.width() - source.width() * sx) / 2.0;
    let y = target.y0 + (target.height() - source.height() * sy) / 2.0;
    for point in ink.strokes.iter_mut().flat_map(|stroke| &mut stroke.points) {
        point[0] = x + (point[0] - source.x0) * sx;
        point[1] = y + (point[1] - source.y0) * sy;
        if !(point[0] as f32).is_finite() || !(point[1] as f32).is_finite() {
            return Err(err(
                op_index,
                "payload.strokes",
                ErrorCode::RectInvalid,
                "fitted signature points cannot be represented in the PDF",
            ));
        }
    }
    ink.stroke_width = Some(scaled_width);
    ink.base.rect = Some(target);
    Ok(())
}
