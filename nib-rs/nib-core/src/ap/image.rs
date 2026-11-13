//! Image → XObject embedding (`/Subtype /Image`). JPEG gray/RGB passes
//! through with `/DCTDecode`; everything else is decoded, downsampled to a
//! max side of 2048 px, and emitted as Flate RGB samples with an `/SMask`
//! gray stream when any alpha < 255 is present.

use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, ExtendedColorType, ImageEncoder, ImageFormat};

use super::{ApError, ImageXObject};

const MAX_SIDE: u32 = 2048;

fn image_dict(w: u32, h: u32, cs: &str, extra: &[(&str, lopdf::Object)]) -> lopdf::Dictionary {
    let mut d = lopdf::Dictionary::new();
    d.set("Type", lopdf::Object::Name(b"XObject".to_vec()));
    d.set("Subtype", lopdf::Object::Name(b"Image".to_vec()));
    d.set("Width", lopdf::Object::Integer(w as i64));
    d.set("Height", lopdf::Object::Integer(h as i64));
    d.set("ColorSpace", lopdf::Object::Name(cs.as_bytes().to_vec()));
    d.set("BitsPerComponent", lopdf::Object::Integer(8));
    for (k, v) in extra {
        d.set(*k, v.clone());
    }
    d
}

/// Embed image bytes (any format `image` can guess) as an image XObject.
/// Returns the XObject and the decoded pixel dimensions.
pub fn embed_image(bytes: &[u8]) -> Result<(ImageXObject, u32, u32), ApError> {
    let err = |e: &dyn std::fmt::Display| ApError::Image(e.to_string());
    match image::guess_format(bytes).map_err(|e| err(&e))? {
        ImageFormat::Jpeg => {
            // Passthrough only when the JPEG is baseline 8-bit gray/RGB.
            let img = image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)
                .map_err(|e| err(&e))?;
            let (w, h) = (img.width(), img.height());
            match img.color() {
                ColorType::L8 | ColorType::Rgb8 => {
                    let cs = if img.color() == ColorType::L8 {
                        "DeviceGray"
                    } else {
                        "DeviceRGB"
                    };
                    let dict = image_dict(
                        w,
                        h,
                        cs,
                        &[("Filter", lopdf::Object::Name(b"DCTDecode".to_vec()))],
                    );
                    let stream = lopdf::Stream::new(dict, bytes.to_vec()).with_compression(false);
                    Ok((
                        ImageXObject {
                            stream,
                            smask: None,
                        },
                        w,
                        h,
                    ))
                }
                _ => embed_decoded(&img),
            }
        }
        _ => embed_decoded(&image::load_from_memory(bytes).map_err(|e| err(&e))?),
    }
}

fn embed_decoded(img: &image::DynamicImage) -> Result<(ImageXObject, u32, u32), ApError> {
    let mut img = img.clone();
    if img.width().max(img.height()) > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let has_alpha = rgba.pixels().any(|p| p[3] < 255);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let mut gray = Vec::with_capacity((w * h) as usize);
    for p in rgba.pixels() {
        rgb.extend_from_slice(&p.0[..3]);
        gray.push(p[3]);
    }
    let mut stream =
        lopdf::Stream::new(image_dict(w, h, "DeviceRGB", &[]), rgb).with_compression(true);
    stream
        .compress()
        .map_err(|e| ApError::Image(e.to_string()))?;
    let smask = if has_alpha {
        let mut s =
            lopdf::Stream::new(image_dict(w, h, "DeviceGray", &[]), gray).with_compression(true);
        s.compress().map_err(|e| ApError::Image(e.to_string()))?;
        Some(s)
    } else {
        None
    };
    Ok((ImageXObject { stream, smask }, w, h))
}

/// Test helper: encode `img` as JPEG (kept out of `#[cfg(test)]` so the
/// feature set stays identical in tests).
#[doc(hidden)]
pub fn encode_jpeg(img: &image::DynamicImage) -> Vec<u8> {
    let mut out = Vec::new();
    JpegEncoder::new(&mut out)
        .write_image(
            img.as_bytes(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgb8,
        )
        .expect("jpeg encode");
    out
}
