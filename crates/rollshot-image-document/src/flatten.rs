//! Flatten committed annotations onto a copy of the full-resolution source
//! (spec §11.2). Selection, hover, viewport, and drafts never reach this
//! module — it consumes only the committed annotation graph.

use image::RgbaImage;

use crate::annotation::Annotation;
use crate::geometry::ImagePoint;
use crate::raster::{
    fill_box_shape, fill_circle, fill_rect, fill_triangle, stroke_box_shape, stroke_circle,
    stroke_line,
};
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
        RenderShape::Line {
            start,
            end,
            width,
            color,
        } => stroke_line(img, *start, *end, *width, *color),
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
        RenderShape::Box {
            kind,
            bounds,
            stroke,
            stroke_width,
            fill,
        } => {
            if let Some(fill_color) = fill {
                fill_box_shape(img, *kind, *bounds, *fill_color);
            }
            stroke_box_shape(img, *kind, *bounds, *stroke_width, *stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry::{ImagePoint, ImageRect};
    use crate::{ImageDocument, Rgb8, StrokeStyle, TwoPointKind};
    use image::{Rgba, RgbaImage};

    fn base(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255]))
    }

    #[test]
    fn flatten_paints_shaft_and_arrowhead_without_mutating_source() {
        let mut doc = ImageDocument::new(base(140, 100));
        doc.add_two_point(
            TwoPointKind::Arrow,
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(110.0, 50.0),
        )
        .unwrap();
        let out = doc.flatten();
        assert_ne!(out.get_pixel(50, 50), doc.source().get_pixel(50, 50));
        assert_ne!(out.get_pixel(100, 50), doc.source().get_pixel(100, 50));
        assert_eq!(doc.source().get_pixel(50, 50).0, [10, 20, 30, 255]);
    }

    #[test]
    fn line_opacity_blends_once_at_full_coverage() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_two_point_with_style(
            TwoPointKind::Line,
            ImagePoint::new(10.0, 50.0),
            ImagePoint::new(90.0, 50.0),
            StrokeStyle {
                color: Rgb8::new(110, 120, 130),
                width: 4.0,
                opacity: 0.5,
            },
        )
        .unwrap();
        assert_eq!(doc.flatten().get_pixel(50, 50).0, [60, 70, 80, 255]);
    }

    #[test]
    fn line_raster_clips_cleanly_at_image_edges() {
        let mut doc = ImageDocument::new(base(30, 20));
        doc.add_two_point(
            TwoPointKind::Line,
            ImagePoint::new(0.0, 0.0),
            ImagePoint::new(20.0, 0.0),
        )
        .unwrap();
        assert_ne!(doc.flatten().get_pixel(0, 0).0, [10, 20, 30, 255]);
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
    fn hundred_mixed_annotations_on_long_image_include_line_and_arrow() {
        let mut doc = ImageDocument::new(base(1000, 20_000));
        // 14 rows × 7 types = 98, plus 1 Rectangle + 1 Ellipse = exactly 100.
        for i in 0..14u32 {
            let y = 100.0 + i as f32 * 950.0;
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
            doc.add_two_point(
                TwoPointKind::Line,
                ImagePoint::new(20.0, y + 100.0),
                ImagePoint::new(300.0, y + 180.0),
            )
            .unwrap();
            doc.add_two_point(
                TwoPointKind::Arrow,
                ImagePoint::new(500.0, y + 100.0),
                ImagePoint::new(900.0, y + 180.0),
            )
            .unwrap();
            doc.add_shape(
                crate::annotation::ShapeKind::Rectangle,
                ImageRect {
                    x: 700.0,
                    y: y + 200.0,
                    width: 60.0,
                    height: 80.0,
                },
            )
            .unwrap();
            doc.add_shape(
                crate::annotation::ShapeKind::Ellipse,
                ImageRect {
                    x: 850.0,
                    y: y + 200.0,
                    width: 60.0,
                    height: 80.0,
                },
            )
            .unwrap();
        }
        // Extra Rectangle + Ellipse to reach exactly 100.
        let ey = 100.0 + 14.0 * 950.0;
        doc.add_shape(
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 100.0,
                y: ey,
                width: 60.0,
                height: 80.0,
            },
        )
        .unwrap();
        let ey2 = 100.0 + 15.0 * 950.0;
        doc.add_shape(
            crate::annotation::ShapeKind::Ellipse,
            ImageRect {
                x: 300.0,
                y: ey2,
                width: 60.0,
                height: 80.0,
            },
        )
        .unwrap();

        assert_eq!(doc.navigator_items().len(), 100);
        let flattened = doc.flatten();
        assert_eq!(flattened.dimensions(), doc.source().dimensions());
        assert_eq!(
            flattened.dimensions(),
            doc.source().dimensions(),
            "output dimensions match source"
        );

        // Representative Number (row 0) still paints.
        assert_ne!(
            flattened.get_pixel(160, 240),
            doc.source().get_pixel(160, 240)
        );

        // Representative Rectangle (row 0): no fill, stroke paints the edge.
        // Row 0 y=100, shape at y+200=300. Bounds: (700, 300)–(760, 380).
        // Interior (730, 340) must be source.
        assert_eq!(
            flattened.get_pixel(730, 340),
            doc.source().get_pixel(730, 340),
            "Rectangle interior without fill must be source"
        );
        // Left edge (700, 340) must be painted by stroke.
        assert_ne!(
            flattened.get_pixel(700, 340),
            doc.source().get_pixel(700, 340),
            "Rectangle edge must be painted by stroke"
        );

        // Representative Ellipse (row 0): no fill, stroke paints the boundary.
        // Row 0 y=100, shape at y+200=300. Bounds: (850, 300)–(910, 380).
        // Center (880, 340) must be source.
        assert_eq!(
            flattened.get_pixel(880, 340),
            doc.source().get_pixel(880, 340),
            "Ellipse center without fill must be source"
        );
        // Top of ellipse (880, 300) must be painted by stroke.
        assert_ne!(
            flattened.get_pixel(880, 300),
            doc.source().get_pixel(880, 300),
            "Ellipse boundary must be painted by stroke"
        );

        assert!(doc.hit_test(ImagePoint::new(160.0, 240.0), 8.0).is_some());
    }

    #[test]
    fn shape_fill_paints_over_source() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_shape_with_style(
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            crate::style::StrokeStyle {
                color: Rgb8::new(0, 0, 0),
                width: 1.0,
                opacity: 1.0,
            },
            Some(Rgb8::new(255, 0, 0)),
        )
        .unwrap();
        let out = doc.flatten();
        // Center of shape should be red (fill)
        let px = out.get_pixel(20, 20).0;
        assert!(
            px[0] > 200 && px[1] < 50 && px[2] < 50,
            "fill should be red"
        );
        // Source pixel should be unchanged
        assert_eq!(doc.source().get_pixel(20, 20).0, [10, 20, 30, 255]);
    }

    #[test]
    fn shape_stroke_only_has_no_fill() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_shape(
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 80.0,
            },
        )
        .unwrap();
        let out = doc.flatten();
        // Center should be unchanged (no fill, stroke is only at edges)
        assert_eq!(out.get_pixel(50, 50).0, [10, 20, 30, 255]);
        // Edge should be painted
        assert_ne!(out.get_pixel(10, 50).0, [10, 20, 30, 255]);
    }

    #[test]
    fn shape_flatten_is_deterministic() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_shape(
            crate::annotation::ShapeKind::Ellipse,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 60.0,
            },
        )
        .unwrap();
        let first = doc.flatten();
        let second = doc.flatten();
        assert_eq!(first.as_raw(), second.as_raw());
    }

    #[test]
    fn opaque_redaction_over_shape_retains_opaque_black() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_shape_with_style(
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 80.0,
            },
            crate::style::StrokeStyle::default(),
            Some(Rgb8::new(255, 0, 0)),
        )
        .unwrap();
        doc.add_redaction(ImageRect {
            x: 20.0,
            y: 20.0,
            width: 20.0,
            height: 20.0,
        })
        .unwrap();
        let out = doc.flatten();
        // Redaction should be opaque black over the shape
        assert_eq!(out.get_pixel(30, 30).0, [0, 0, 0, 255]);
    }
}
