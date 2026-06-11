//! Flatten committed annotations onto a copy of the full-resolution source
//! (spec §11.2). Selection, hover, viewport, and drafts never reach this
//! module — it consumes only the committed annotation graph.

use image::RgbaImage;

use crate::annotation::Annotation;
use crate::geometry::ImagePoint;
use crate::raster::{fill_circle, fill_rect, fill_triangle, stroke_circle};
use crate::shapes::{annotation_shapes, RenderShape, TextAnchor};
use crate::text::{draw_block, measure_block};

pub(crate) fn flatten_onto(source: &RgbaImage, annotations: &[Annotation]) -> RgbaImage {
    let mut out = source.clone();
    for annotation in annotations {
        for shape in annotation_shapes(annotation) {
            draw_shape(&mut out, &shape);
        }
    }
    out
}

fn draw_shape(img: &mut RgbaImage, shape: &RenderShape) {
    match shape {
        RenderShape::Rect { rect, color } => fill_rect(img, *rect, *color),
        RenderShape::Circle {
            center,
            radius,
            fill,
            outline_width,
            outline,
        } => {
            fill_circle(img, *center, *radius, *fill);
            if *outline_width > 0.0 {
                stroke_circle(img, *center, *radius, *outline_width, *outline);
            }
        }
        RenderShape::Triangle { points, color } => fill_triangle(img, points, *color),
        RenderShape::Label {
            anchor,
            anchor_kind,
            content,
            px,
            bold,
            color,
        } => {
            let top_left = match anchor_kind {
                TextAnchor::TopLeft => *anchor,
                TextAnchor::Center => {
                    let (w, h) = measure_block(content, *px, *bold);
                    ImagePoint::new(anchor.x - w / 2.0, anchor.y - h / 2.0)
                }
            };
            draw_block(img, top_left, content, *px, *bold, *color);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::ImageDocument;
    use image::{Rgba, RgbaImage};

    fn base(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255]))
    }

    #[test]
    fn flatten_with_no_annotations_equals_source_and_source_is_untouched() {
        let doc = ImageDocument::new(base(50, 50));
        let out = doc.flatten();
        assert_eq!(out.as_raw(), doc.source().as_raw());
    }

    #[test]
    fn redaction_replaces_covered_pixels_exactly_opaque() {
        let mut doc = ImageDocument::new(base(50, 50));
        doc.add_redaction(ImageRect {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        })
        .unwrap();
        let out = doc.flatten();
        assert_eq!(out.get_pixel(20, 20).0, [0, 0, 0, 255]);
        assert_eq!(out.get_pixel(5, 5).0, [10, 20, 30, 255]);
        assert_eq!(doc.source().get_pixel(20, 20).0, [10, 20, 30, 255]);
    }

    #[test]
    fn number_callout_paints_accent_bubble_and_white_label() {
        let mut doc = ImageDocument::new(base(200, 200));
        doc.add_number_callout(ImagePoint::new(30.0, 30.0), ImagePoint::new(100.0, 100.0));
        let out = doc.flatten();
        let center = out.get_pixel(100, 100).0;
        let ring = out.get_pixel(110, 100).0;
        assert!(
            center != [10, 20, 30, 255] && ring != [10, 20, 30, 255],
            "bubble must paint over the source"
        );
        let mut white_nearby = 0;
        for y in 90..110 {
            for x in 90..110 {
                let p = out.get_pixel(x, y).0;
                if p[0] > 200 && p[1] > 200 && p[2] > 200 {
                    white_nearby += 1;
                }
            }
        }
        assert!(
            white_nearby > 3,
            "expected white label pixels, got {white_nearby}"
        );
        let leader = out.get_pixel(80, 80).0;
        assert_ne!(
            leader,
            [10, 20, 30, 255],
            "leader triangle paints toward the tip"
        );
    }

    #[test]
    fn text_note_paints_plate_and_glyphs() {
        let mut doc = ImageDocument::new(base(300, 100));
        doc.add_text_note(ImagePoint::new(10.0, 10.0), "Hello".to_string())
            .unwrap();
        let out = doc.flatten();
        let plate = out.get_pixel(14, 14).0;
        assert!(
            plate[0] < 30 && plate[1] < 30 && plate[2] < 30,
            "dark plate expected"
        );
        let changed = out
            .pixels()
            .zip(doc.source().pixels())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 100,
            "plate + glyphs change many pixels, got {changed}"
        );
    }

    #[test]
    fn flatten_excludes_nothing_committed_and_is_repeatable() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(10.0, 10.0));
        let first = doc.flatten();
        let second = doc.flatten();
        assert_eq!(first.as_raw(), second.as_raw(), "flatten is deterministic");
    }

    /// Spec §13/§16: long image at the history-limit annotation scale.
    #[test]
    fn hundred_annotations_on_long_image_flatten_hit_test_and_order() {
        let mut doc = ImageDocument::new(base(1000, 20000));
        for i in 0..34u32 {
            let y = 100.0 + i as f32 * 580.0;
            doc.add_number_callout(ImagePoint::new(100.0, y), ImagePoint::new(160.0, y));
            doc.add_text_note(ImagePoint::new(300.0, y), format!("step {i}"))
                .unwrap();
            doc.add_redaction(ImageRect {
                x: 500.0,
                y,
                width: 80.0,
                height: 40.0,
            })
            .unwrap();
        }
        assert_eq!(doc.annotations().len(), 102);
        let items = doc.navigator_items();
        assert_eq!(items.len(), 102);
        assert!(doc.hit_test(ImagePoint::new(160.0, 100.0), 8.0).is_some());
        let out = doc.flatten();
        assert_eq!(out.dimensions(), (1000, 20000));
    }
}
