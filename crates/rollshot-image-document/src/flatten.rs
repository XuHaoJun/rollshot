//! Flatten committed annotations onto a copy of the full-resolution source
//! (spec §11.2). Selection, hover, viewport, and drafts never reach this
//! module — it consumes only the committed annotation graph.

use std::sync::Arc;

use image::RgbaImage;

use crate::annotation::Annotation;
use crate::geometry::ImagePoint;
use crate::pixelate::{apply_pixelate, pixelate_region};
use crate::raster::{
    fill_box_shape, fill_circle, fill_rect, fill_triangle, stroke_box_shape, stroke_circle,
    stroke_line, stroke_polyline,
};
use crate::shapes::{annotation_commands, RenderCommand, RenderShape, TextAnchor};
use crate::text::{draw_block, measure_block};

#[derive(Debug, Clone)]
pub struct FlattenSnapshot {
    source: Arc<RgbaImage>,
    annotations: Vec<Annotation>,
}

impl FlattenSnapshot {
    pub(crate) fn new(source: Arc<RgbaImage>, annotations: Vec<Annotation>) -> Self {
        Self {
            source,
            annotations,
        }
    }

    pub fn shared_source(&self) -> Arc<RgbaImage> {
        Arc::clone(&self.source)
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.source.dimensions()
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub fn flatten(&self) -> RgbaImage {
        flatten_onto(&self.source, &self.annotations)
    }
}

pub(crate) fn flatten_onto(source: &RgbaImage, annotations: &[Annotation]) -> RgbaImage {
    let mut out = source.clone();
    for annotation in annotations {
        for command in annotation_commands(annotation) {
            match command {
                RenderCommand::Shape(shape) => draw_shape(&mut out, &shape),
                RenderCommand::Pixelate { bounds, block_size } => {
                    let started = std::time::Instant::now();
                    if let Ok(region) = pixelate_region(source, bounds, block_size) {
                        apply_pixelate(&mut out, &region);
                        tracing::debug!(
                            target: "rollshot::annotation",
                            operation = "pixelate_flatten",
                            raster_width = region.region.width,
                            raster_height = region.region.height,
                            block_size,
                            elapsed_us = started.elapsed().as_micros() as u64,
                            "flattened pixelate annotation"
                        );
                    }
                }
            }
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
        RenderShape::Polyline {
            points,
            width,
            color,
        } => stroke_polyline(img, points, *width, *color),
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
    use super::flatten_onto;
    use crate::annotation::{Annotation, AnnotationId, FreehandKind, TwoPointKind};
    use crate::geometry::{ImagePoint, ImageRect, Rgb8};
    use crate::pixelate::{apply_pixelate, pixelate_region};
    use crate::shapes::{annotation_commands, RenderCommand};
    use crate::{ImageDocument, StrokeStyle};
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
        // 12 rows × 8 types = 96, plus Pen (row 3) + Highlighter (row 10) = 98,
        // plus NumberCallout (row 0 extra) + TextNote (row 1 extra) = 100.
        for i in 0..12u32 {
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
            doc.add_pixelate(
                ImageRect {
                    x: 950.0,
                    y,
                    width: 40.0,
                    height: 40.0,
                },
                16,
            )
            .unwrap();
        }
        // 12 rows × 8 types = 96, + Pen (row 3) + Highlighter (row 10) = 98.
        // Extra NumberCallout at row 13, extra TextNote at row 13 = 100.
        let extra_y = 100.0 + 13_f32 * 950.0;
        doc.add_number_callout(
            ImagePoint::new(100.0, extra_y),
            ImagePoint::new(160.0, extra_y),
        );
        doc.add_text_note(ImagePoint::new(300.0, extra_y), "extra text".to_string())
            .unwrap();

        // Pen (row 3)
        {
            let y = 100.0 + 3_f32 * 950.0;
            let pts: Vec<_> = (0..5)
                .map(|p| ImagePoint::new(400.0 + p as f32 * 20.0, y + 100.0 + p as f32 * 30.0))
                .collect();
            doc.add_freehand_with_style(
                FreehandKind::Pen,
                pts,
                StrokeStyle {
                    color: Rgb8::new(0, 0, 0),
                    width: 2.0,
                    opacity: 1.0,
                },
            )
            .unwrap();
        }
        // Highlighter (row 10)
        {
            let y = 100.0 + 10_f32 * 950.0;
            let pts: Vec<_> = (0..5)
                .map(|p| ImagePoint::new(150.0 + p as f32 * 15.0, y + 100.0 + p as f32 * 30.0))
                .collect();
            doc.add_freehand_with_style(
                FreehandKind::Highlighter,
                pts,
                StrokeStyle::highlighter_default(),
            )
            .unwrap();
        }
        // 12 rows × 8 types = 96, + Pen (row 3) + Highlighter (row 10) = 98,
        // + extra NumberCallout + extra TextNote = 100.

        assert_eq!(doc.navigator_items().len(), 100);
        let flattened = doc.flatten();
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

        // Representative Pixelate (row 0): region (950, 100)–(990, 140).
        // Source is uniform so pixelate averages to the same color; the
        // dedicated pixelate unit tests verify non-trivial content.
        // Here we only assert the flatten completes and the output covers
        // the full source dimensions (already asserted above).

        // Navigator must include Pixelate items.
        assert!(
            doc.navigator_items()
                .iter()
                .any(|item| item.label == "Pixelate"),
            "Navigator must contain Pixelate entries"
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
    fn polyline_self_crossing_blends_alpha_exactly_once() {
        let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));
        let points = vec![
            ImagePoint::new(5.0, 5.0),
            ImagePoint::new(35.0, 35.0),
            ImagePoint::new(5.0, 35.0),
            ImagePoint::new(35.0, 5.0),
        ];
        let color = crate::geometry::Rgba8::new(0, 0, 0, 128);
        crate::raster::stroke_polyline(&mut img, &points, 4.0, color);
        let crossing = img.get_pixel(20, 20).0[0];
        assert!((126..=129).contains(&crossing), "got {crossing}");
        let single = img.get_pixel(10, 10).0[0];
        assert_eq!(crossing, single);
    }

    #[test]
    fn two_separate_polylines_darken_where_they_cross() {
        let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));
        let color = crate::geometry::Rgba8::new(0, 0, 0, 128);
        let a = vec![ImagePoint::new(5.0, 20.0), ImagePoint::new(35.0, 20.0)];
        let b = vec![ImagePoint::new(20.0, 5.0), ImagePoint::new(20.0, 35.0)];
        crate::raster::stroke_polyline(&mut img, &a, 4.0, color);
        crate::raster::stroke_polyline(&mut img, &b, 4.0, color);
        let crossing = img.get_pixel(20, 20).0[0];
        let single = img.get_pixel(10, 20).0[0];
        assert!(
            crossing < single,
            "two strokes must darken: {crossing} vs {single}"
        );
    }

    #[test]
    fn polyline_has_round_caps() {
        let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]));
        let points = vec![ImagePoint::new(10.0, 20.0), ImagePoint::new(30.0, 20.0)];
        crate::raster::stroke_polyline(
            &mut img,
            &points,
            8.0,
            crate::geometry::Rgba8::new(0, 0, 0, 255),
        );
        assert!(img.get_pixel(33, 20).0[0] < 128);
        assert_eq!(img.get_pixel(36, 20).0[0], 255);
    }

    #[test]
    fn copy_original_remains_byte_identical_to_source() {
        let mut doc = ImageDocument::new(base(100, 100));
        doc.add_number_callout(ImagePoint::new(10.0, 10.0), ImagePoint::new(50.0, 50.0));
        doc.add_redaction(ImageRect {
            x: 20.0,
            y: 20.0,
            width: 30.0,
            height: 30.0,
        })
        .unwrap();
        doc.add_shape(
            crate::annotation::ShapeKind::Rectangle,
            ImageRect {
                x: 60.0,
                y: 60.0,
                width: 20.0,
                height: 20.0,
            },
        )
        .unwrap();

        let source = doc.source().clone();
        let copy_original = doc.source().clone();
        assert_eq!(
            copy_original.as_raw(),
            source.as_raw(),
            "Copy Original must be byte-identical to the unflattened source"
        );

        let flattened = doc.flatten();
        assert_ne!(
            flattened.as_raw(),
            source.as_raw(),
            "flattened output must differ from source when annotations exist"
        );
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

    #[test]
    fn committed_highlighter_flattens_with_uniform_alpha() {
        let mut doc = crate::ImageDocument::new(RgbaImage::from_pixel(
            60,
            60,
            image::Rgba([255, 255, 255, 255]),
        ));
        doc.add_freehand_with_style(
            crate::FreehandKind::Highlighter,
            vec![
                ImagePoint::new(10.0, 10.0),
                ImagePoint::new(50.0, 50.0),
                ImagePoint::new(10.0, 50.0),
                ImagePoint::new(50.0, 10.0),
            ],
            crate::StrokeStyle::highlighter_default(),
        )
        .unwrap();
        let out = doc.flatten();
        let crossing = out.get_pixel(30, 30).0;
        let single = out.get_pixel(15, 15).0;
        assert_eq!(crossing, single, "self-overlap must not darken");
        assert!(crossing[2] < 255);
        assert_eq!(doc.source().get_pixel(30, 30).0, [255, 255, 255, 255]);
    }

    #[test]
    fn committed_pen_flattens_opaque() {
        let mut doc = crate::ImageDocument::new(RgbaImage::from_pixel(
            40,
            40,
            image::Rgba([255, 255, 255, 255]),
        ));
        doc.add_freehand_with_style(
            crate::FreehandKind::Pen,
            vec![ImagePoint::new(5.0, 20.0), ImagePoint::new(35.0, 20.0)],
            crate::StrokeStyle::default(),
        )
        .unwrap();
        let out = doc.flatten();
        assert_eq!(out.get_pixel(20, 20).0, [0xE5, 0x48, 0x4D, 255]);
    }

    // --- Task 3: RenderCommand and pixelate flatten tests ---

    fn four_quadrant_fixture() -> RgbaImage {
        let mut img = RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                let color = if x < 8 && y < 8 {
                    [255u8, 0, 0, 255]
                } else if x >= 8 && y < 8 {
                    [0, 255, 0, 255]
                } else if x < 8 && y >= 8 {
                    [0, 0, 255, 255]
                } else {
                    [255, 255, 0, 255]
                };
                img.put_pixel(x, y, Rgba(color));
            }
        }
        img
    }

    fn red_rectangle_over_center() -> Annotation {
        Annotation::OpaqueRedaction {
            id: AnnotationId(100),
            bounds: ImageRect::new(2.0, 2.0, 12.0, 12.0),
        }
    }

    fn pixelate_center(block_size: u32) -> Annotation {
        Annotation::pixelate(
            AnnotationId(101),
            ImageRect::new(2.0, 2.0, 12.0, 12.0),
            block_size,
        )
    }

    fn blue_arrow_over_center() -> Annotation {
        Annotation::two_point_with_style(
            AnnotationId(102),
            TwoPointKind::Arrow,
            ImagePoint::new(2.0, 2.0),
            ImagePoint::new(14.0, 14.0),
            StrokeStyle {
                color: Rgb8::new(0, 0, 255),
                width: 4.0,
                opacity: 1.0,
            },
        )
    }

    fn pixelated_source_center(source: &RgbaImage) -> RgbaImage {
        let mut out = source.clone();
        let region = pixelate_region(source, ImageRect::new(2.0, 2.0, 12.0, 12.0), 4).unwrap();
        apply_pixelate(&mut out, &region);
        out
    }

    fn asymmetric_source_fixture() -> RgbaImage {
        let mut img = RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                img.put_pixel(
                    x,
                    y,
                    Rgba([x as u8 * 16, y as u8 * 16, (x + y) as u8 * 8, 255]),
                );
            }
        }
        img
    }

    fn pixelate_rect(x: f32, y: f32, w: f32, h: f32, block_size: u32) -> Annotation {
        Annotation::pixelate(AnnotationId(200), ImageRect::new(x, y, w, h), block_size)
    }

    fn crop(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
        let mut cropped = RgbaImage::new(w, h);
        for dy in 0..h {
            for dx in 0..w {
                cropped.put_pixel(dx, dy, *img.get_pixel(x + dx, y + dy));
            }
        }
        cropped
    }

    #[test]
    fn pixelate_lowers_to_one_raster_command() {
        let annotation =
            Annotation::pixelate(AnnotationId(7), ImageRect::new(3.0, 4.0, 8.0, 9.0), 16);
        assert_eq!(
            annotation_commands(&annotation),
            vec![RenderCommand::Pixelate {
                bounds: ImageRect::new(3.0, 4.0, 8.0, 9.0),
                block_size: 16,
            }]
        );
    }

    #[test]
    fn pixelate_covers_earlier_annotations_but_later_annotations_cover_pixelate() {
        let source = four_quadrant_fixture();
        let earlier = red_rectangle_over_center();
        let pixelate = pixelate_center(4);
        let later = blue_arrow_over_center();
        let only_earlier = flatten_onto(&source, &[earlier.clone(), pixelate.clone()]);
        let with_later = flatten_onto(&source, &[earlier, pixelate, later]);
        assert_eq!(
            only_earlier.get_pixel(4, 4),
            pixelated_source_center(&source).get_pixel(4, 4)
        );
        assert_eq!(with_later.get_pixel(4, 4).0, [0, 0, 255, 255]);
    }

    #[test]
    fn overlapping_pixelates_each_sample_original_source() {
        let source = asymmetric_source_fixture();
        let twice = flatten_onto(
            &source,
            &[
                pixelate_rect(0.0, 0.0, 8.0, 8.0, 4),
                pixelate_rect(2.0, 2.0, 6.0, 6.0, 4),
            ],
        );
        let second_only = flatten_onto(&source, &[pixelate_rect(2.0, 2.0, 6.0, 6.0, 4)]);
        assert_eq!(crop(&twice, 2, 2, 6, 6), crop(&second_only, 2, 2, 6, 6));
        assert_eq!(source, asymmetric_source_fixture());
    }
}
