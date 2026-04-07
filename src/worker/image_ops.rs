use super::*;

pub(super) fn run_transform(request_id: u64, op: TransformOp, image: Arc<DynamicImage>) -> WorkerResult {
    log::debug!(
        target: "imranview::worker",
        "transform start request_id={} op={:?}",
        request_id,
        op
    );
    let started = Instant::now();
    let transformed = apply_transform(op, image.as_ref());

    match transformed {
        Ok(transformed) => {
            let loaded = payload_from_working_image(Arc::new(transformed));
            log_timing("edit_image", started.elapsed(), EDIT_IMAGE_BUDGET);
            WorkerResult::Transformed { request_id, loaded }
        }
        Err(err) => WorkerResult::Failed {
            request_id: Some(request_id),
            kind: WorkerRequestKind::Edit,
            error: err.to_string(),
        },
    }
}

pub(super) fn apply_transform(op: TransformOp, image: &DynamicImage) -> Result<DynamicImage> {
    match op {
        TransformOp::RotateLeft => Ok(image.rotate270()),
        TransformOp::RotateRight => Ok(image.rotate90()),
        TransformOp::FlipHorizontal => Ok(image.fliph()),
        TransformOp::FlipVertical => Ok(image.flipv()),
        TransformOp::AddBorder {
            left,
            right,
            top,
            bottom,
            color,
        } => apply_border(image, left, right, top, bottom, color),
        TransformOp::CanvasSize {
            width,
            height,
            anchor,
            fill,
        } => apply_canvas_size(image, width, height, anchor, fill),
        TransformOp::RotateFine {
            angle_degrees,
            interpolation,
            expand_canvas,
            fill,
        } => apply_rotate_fine(image, angle_degrees, interpolation, expand_canvas, fill),
        TransformOp::AddText {
            text,
            x,
            y,
            scale,
            color,
        } => Ok(apply_text(image, &text, x, y, scale, color)),
        TransformOp::DrawShape(params) => Ok(apply_shape(image, params)),
        TransformOp::OverlayImage {
            overlay_path,
            opacity,
            anchor,
        } => apply_overlay_image(image, &overlay_path, opacity, anchor),
        TransformOp::SelectionWorkflow(params) => apply_selection_workflow(image, params),
        TransformOp::ReplaceColor {
            source,
            target,
            tolerance,
            preserve_alpha,
        } => Ok(apply_replace_color(
            image,
            source,
            target,
            tolerance,
            preserve_alpha,
        )),
        TransformOp::AlphaAdjust {
            alpha_percent,
            alpha_from_luma,
            invert_luma,
            region,
        } => Ok(apply_alpha_adjust(
            image,
            alpha_percent,
            alpha_from_luma,
            invert_luma,
            region,
        )),
        TransformOp::AlphaBrush {
            center_x,
            center_y,
            radius,
            strength_percent,
            softness,
            operation,
        } => Ok(apply_alpha_brush(
            image,
            center_x,
            center_y,
            radius,
            strength_percent,
            softness,
            operation,
        )),
        TransformOp::Effects(params) => Ok(apply_effects(image, params)),
        TransformOp::PerspectiveCorrect {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
            output_width,
            output_height,
            interpolation,
            fill,
        } => apply_perspective_correct(
            image,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
            output_width,
            output_height,
            interpolation,
            fill,
        ),
        TransformOp::Resize {
            width,
            height,
            filter,
        } => {
            if width == 0 || height == 0 {
                return Err(anyhow!("resize dimensions must be greater than zero"));
            }
            Ok(image.resize_exact(width, height, filter.to_image_filter()))
        }
        TransformOp::Crop {
            x,
            y,
            width,
            height,
        } => {
            let (source_width, source_height) = image.dimensions();
            if width == 0 || height == 0 {
                return Err(anyhow!("crop dimensions must be greater than zero"));
            }
            if x >= source_width || y >= source_height {
                return Err(anyhow!("crop origin is outside image bounds"));
            }
            let end_x = x.saturating_add(width);
            let end_y = y.saturating_add(height);
            if end_x > source_width || end_y > source_height {
                return Err(anyhow!("crop rectangle exceeds image bounds"));
            }
            Ok(image.crop_imm(x, y, width, height))
        }
        TransformOp::ColorAdjust(params) => Ok(apply_color_adjustments(image, params)),
    }
}

pub(super) fn apply_text(
    image: &DynamicImage,
    text: &str,
    x: i32,
    y: i32,
    scale: u32,
    color: [u8; 4],
) -> DynamicImage {
    let mut output = image.to_rgba8();
    if text.trim().is_empty() {
        return DynamicImage::ImageRgba8(output);
    }

    let scale = scale.clamp(1, 16);
    let draw_color = Rgba(color);
    let mut cursor_x = x;
    let mut cursor_y = y;
    for ch in text.chars() {
        if ch == '\n' {
            cursor_x = x;
            cursor_y += (8 * scale + scale) as i32;
            continue;
        }
        let Some(glyph) = font8x8::BASIC_FONTS.get(ch) else {
            cursor_x += (8 * scale) as i32;
            continue;
        };

        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8 {
                if (bits >> col) & 1 == 1 {
                    let px = cursor_x + (col as i32 * scale as i32);
                    let py = cursor_y + (row as i32 * scale as i32);
                    fill_rect_blend(&mut output, px, py, scale, scale, draw_color);
                }
            }
        }

        cursor_x += (8 * scale + scale) as i32;
    }

    DynamicImage::ImageRgba8(output)
}

pub(super) fn apply_shape(image: &DynamicImage, params: ShapeParams) -> DynamicImage {
    let mut output = image.to_rgba8();
    let thickness = params.thickness.clamp(1, 128);
    let color = Rgba(params.color);
    match params.kind {
        ShapeKind::Line => draw_line(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            color,
        ),
        ShapeKind::Rectangle => draw_rectangle(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
        ShapeKind::Ellipse => draw_ellipse(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
        ShapeKind::Arrow => draw_arrow(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            color,
        ),
        ShapeKind::RoundedRectangleShadow => draw_rounded_rect_shadow(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
        ShapeKind::SpeechBubble => draw_speech_bubble(
            &mut output,
            params.start_x,
            params.start_y,
            params.end_x,
            params.end_y,
            thickness,
            params.filled,
            color,
        ),
    }

    DynamicImage::ImageRgba8(output)
}

pub(super) fn apply_overlay_image(
    image: &DynamicImage,
    overlay_path: &Path,
    opacity: f32,
    anchor: CanvasAnchor,
) -> Result<DynamicImage> {
    let overlay = image::open(overlay_path)
        .with_context(|| format!("failed to open overlay image {}", overlay_path.display()))?
        .to_rgba8();
    let mut output = image.to_rgba8();
    let (base_w, base_h) = output.dimensions();
    let (ov_w, ov_h) = overlay.dimensions();
    if ov_w == 0 || ov_h == 0 {
        return Err(anyhow!("overlay image is empty"));
    }

    let (factor_x, factor_y) = anchor.factors();
    let dx_max = base_w.saturating_sub(ov_w);
    let dy_max = base_h.saturating_sub(ov_h);
    let offset_x = ((dx_max as f32 * factor_x).round() as u32).min(dx_max);
    let offset_y = ((dy_max as f32 * factor_y).round() as u32).min(dy_max);
    let opacity = opacity.clamp(0.0, 1.0);

    for y in 0..ov_h.min(base_h) {
        for x in 0..ov_w.min(base_w) {
            let target_x = offset_x.saturating_add(x);
            let target_y = offset_y.saturating_add(y);
            if target_x >= base_w || target_y >= base_h {
                continue;
            }
            let mut src = *overlay.get_pixel(x, y);
            src.0[3] = (src.0[3] as f32 * opacity).round().clamp(0.0, 255.0) as u8;
            blend_pixel(output.get_pixel_mut(target_x, target_y), src);
        }
    }
    Ok(DynamicImage::ImageRgba8(output))
}

pub(super) fn apply_selection_workflow(image: &DynamicImage, params: SelectionParams) -> Result<DynamicImage> {
    let source = image.to_rgba8();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return Err(anyhow!("no image pixels to process"));
    }

    match params.workflow {
        SelectionWorkflow::CropRect => {
            if params.width == 0 || params.height == 0 {
                return Err(anyhow!(
                    "selection crop dimensions must be greater than zero"
                ));
            }
            if params.x >= w || params.y >= h {
                return Err(anyhow!("selection origin is outside image bounds"));
            }
            let crop_w = params.width.min(w - params.x);
            let crop_h = params.height.min(h - params.y);
            Ok(DynamicImage::ImageRgba8(
                image::imageops::crop_imm(&source, params.x, params.y, crop_w, crop_h).to_image(),
            ))
        }
        SelectionWorkflow::CutOutsideRect => {
            let mut output = RgbaImage::from_pixel(w, h, Rgba(params.fill));
            if params.width == 0 || params.height == 0 {
                return Ok(DynamicImage::ImageRgba8(output));
            }
            if params.x >= w || params.y >= h {
                return Ok(DynamicImage::ImageRgba8(output));
            }
            let copy_w = params.width.min(w - params.x);
            let copy_h = params.height.min(h - params.y);
            for y in 0..copy_h {
                for x in 0..copy_w {
                    output.put_pixel(
                        params.x + x,
                        params.y + y,
                        *source.get_pixel(params.x + x, params.y + y),
                    );
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CropCircle => {
            if params.radius == 0 {
                return Err(anyhow!("circle radius must be greater than zero"));
            }
            let diameter = params.radius.saturating_mul(2);
            let mut output = RgbaImage::from_pixel(diameter, diameter, Rgba([0, 0, 0, 0]));
            let cx = params.x as i32;
            let cy = params.y as i32;
            let radius = params.radius as i32;
            for oy in 0..diameter {
                for ox in 0..diameter {
                    let dx = ox as i32 - radius;
                    let dy = oy as i32 - radius;
                    if dx * dx + dy * dy <= radius * radius {
                        let sx = cx + dx;
                        let sy = cy + dy;
                        if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                            output.put_pixel(ox, oy, *source.get_pixel(sx as u32, sy as u32));
                        }
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CutOutsideCircle => {
            if params.radius == 0 {
                return Err(anyhow!("circle radius must be greater than zero"));
            }
            let mut output = RgbaImage::from_pixel(w, h, Rgba(params.fill));
            let cx = params.x as i32;
            let cy = params.y as i32;
            let radius = params.radius as i32;
            for y in 0..h {
                for x in 0..w {
                    let dx = x as i32 - cx;
                    let dy = y as i32 - cy;
                    if dx * dx + dy * dy <= radius * radius {
                        output.put_pixel(x, y, *source.get_pixel(x, y));
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CropPolygon => {
            if params.polygon_points.len() < 3 {
                return Err(anyhow!("polygon crop requires at least 3 points"));
            }
            let points: Vec<(i32, i32)> = params
                .polygon_points
                .iter()
                .map(|p| (p[0] as i32, p[1] as i32))
                .collect();
            let (min_x, max_x, min_y, max_y) = polygon_bounds(&points)?;
            if min_x < 0 || min_y < 0 || min_x as u32 >= w || min_y as u32 >= h {
                return Err(anyhow!("polygon origin is outside image bounds"));
            }
            let out_w = (max_x - min_x + 1).max(1) as u32;
            let out_h = (max_y - min_y + 1).max(1) as u32;
            let mut output = RgbaImage::from_pixel(out_w, out_h, Rgba([0, 0, 0, 0]));
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
                        continue;
                    }
                    if point_in_polygon(x, y, &points) {
                        output.put_pixel(
                            (x - min_x) as u32,
                            (y - min_y) as u32,
                            *source.get_pixel(x as u32, y as u32),
                        );
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
        SelectionWorkflow::CutOutsidePolygon => {
            if params.polygon_points.len() < 3 {
                return Err(anyhow!("polygon cut-outside requires at least 3 points"));
            }
            let points: Vec<(i32, i32)> = params
                .polygon_points
                .iter()
                .map(|p| (p[0] as i32, p[1] as i32))
                .collect();
            let mut output = RgbaImage::from_pixel(w, h, Rgba(params.fill));
            for y in 0..h {
                for x in 0..w {
                    if point_in_polygon(x as i32, y as i32, &points) {
                        output.put_pixel(x, y, *source.get_pixel(x, y));
                    }
                }
            }
            Ok(DynamicImage::ImageRgba8(output))
        }
    }
}

pub(super) fn polygon_bounds(points: &[(i32, i32)]) -> Result<(i32, i32, i32, i32)> {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for &(x, y) in points {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if min_x > max_x || min_y > max_y {
        return Err(anyhow!("invalid polygon bounds"));
    }
    Ok((min_x, max_x, min_y, max_y))
}

pub(super) fn point_in_polygon(x: i32, y: i32, points: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) {
            let x_intersect = ((xj - xi) as f32 * (y - yi) as f32) / (yj - yi) as f32 + xi as f32;
            if (x as f32) < x_intersect {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

pub(super) fn apply_replace_color(
    image: &DynamicImage,
    source: [u8; 4],
    target: [u8; 4],
    tolerance: u8,
    preserve_alpha: bool,
) -> DynamicImage {
    let mut output = image.to_rgba8();
    let src = source;
    let distance_max = tolerance as i32 * tolerance as i32 * 3;
    for px in output.pixels_mut() {
        let dr = px[0] as i32 - src[0] as i32;
        let dg = px[1] as i32 - src[1] as i32;
        let db = px[2] as i32 - src[2] as i32;
        let dist = dr * dr + dg * dg + db * db;
        if dist <= distance_max {
            px[0] = target[0];
            px[1] = target[1];
            px[2] = target[2];
            px[3] = if preserve_alpha { px[3] } else { target[3] };
        }
    }
    DynamicImage::ImageRgba8(output)
}

pub(super) fn apply_alpha_adjust(
    image: &DynamicImage,
    alpha_percent: f32,
    alpha_from_luma: bool,
    invert_luma: bool,
    region: Option<(u32, u32, u32, u32)>,
) -> DynamicImage {
    let mut output = image.to_rgba8();
    let (w, h) = output.dimensions();
    let bounds = region.map(|(x, y, width, height)| {
        let x0 = x.min(w);
        let y0 = y.min(h);
        let x1 = x0.saturating_add(width).min(w);
        let y1 = y0.saturating_add(height).min(h);
        (x0, y0, x1, y1)
    });
    let factor = (alpha_percent / 100.0).clamp(0.0, 1.0);
    for y in 0..h {
        for x in 0..w {
            if let Some((x0, y0, x1, y1)) = bounds {
                if x < x0 || x >= x1 || y < y0 || y >= y1 {
                    continue;
                }
            }
            let px = output.get_pixel_mut(x, y);
            let mut alpha = px[3] as f32 * factor;
            if alpha_from_luma {
                let luma = 0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32;
                alpha = if invert_luma { 255.0 - luma } else { luma };
            }
            px[3] = alpha.round().clamp(0.0, 255.0) as u8;
        }
    }
    DynamicImage::ImageRgba8(output)
}

pub(super) fn apply_alpha_brush(
    image: &DynamicImage,
    center_x: u32,
    center_y: u32,
    radius: u32,
    strength_percent: f32,
    softness: f32,
    operation: AlphaBrushOp,
) -> DynamicImage {
    let mut output = image.to_rgba8();
    let (w, h) = output.dimensions();
    if w == 0 || h == 0 {
        return DynamicImage::ImageRgba8(output);
    }

    let radius = radius.max(1);
    let radius_f = radius as f32;
    let strength = (strength_percent / 100.0).clamp(0.0, 1.0);
    if strength <= 0.0 {
        return DynamicImage::ImageRgba8(output);
    }
    let softness = softness.clamp(0.0, 1.0);
    let inner_radius = radius_f * (1.0 - softness);

    let min_x = center_x.saturating_sub(radius);
    let min_y = center_y.saturating_sub(radius);
    let max_x = center_x.saturating_add(radius).min(w.saturating_sub(1));
    let max_y = center_y.saturating_add(radius).min(h.saturating_sub(1));

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 - center_x as f32;
            let dy = y as f32 - center_y as f32;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > radius_f {
                continue;
            }
            let falloff = if distance <= inner_radius || inner_radius >= radius_f {
                1.0
            } else {
                let t = ((distance - inner_radius) / (radius_f - inner_radius)).clamp(0.0, 1.0);
                (1.0 - t).powf(1.4)
            };
            let weight = (strength * falloff).clamp(0.0, 1.0);
            if weight <= 0.0 {
                continue;
            }

            let px = output.get_pixel_mut(x, y);
            let current_alpha = px[3] as f32;
            let next_alpha = match operation {
                AlphaBrushOp::Increase | AlphaBrushOp::SetOpaque => {
                    current_alpha + (255.0 - current_alpha) * weight
                }
                AlphaBrushOp::Decrease | AlphaBrushOp::SetTransparent => {
                    current_alpha * (1.0 - weight)
                }
            };
            px[3] = next_alpha.round().clamp(0.0, 255.0) as u8;
        }
    }

    DynamicImage::ImageRgba8(output)
}

pub(super) fn apply_effects(image: &DynamicImage, params: EffectsParams) -> DynamicImage {
    let mut current = image.clone();
    if params.blur_sigma > 0.01 {
        current = current.blur(params.blur_sigma.clamp(0.0, 30.0));
    }
    if params.sharpen_sigma > 0.01 {
        current = current.unsharpen(
            params.sharpen_sigma.clamp(0.0, 30.0),
            params.sharpen_threshold.clamp(-255, 255),
        );
    }
    if params.grayscale {
        current = current.grayscale();
    }

    let mut rgba = current.to_rgba8();
    if params.invert {
        for px in rgba.pixels_mut() {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
        }
    }

    if params.sepia_strength > 0.001 {
        let strength = params.sepia_strength.clamp(0.0, 1.0);
        for px in rgba.pixels_mut() {
            let r = px[0] as f32;
            let g = px[1] as f32;
            let b = px[2] as f32;
            let sepia_r = (0.393 * r + 0.769 * g + 0.189 * b).clamp(0.0, 255.0);
            let sepia_g = (0.349 * r + 0.686 * g + 0.168 * b).clamp(0.0, 255.0);
            let sepia_b = (0.272 * r + 0.534 * g + 0.131 * b).clamp(0.0, 255.0);
            px[0] = (r * (1.0 - strength) + sepia_r * strength).round() as u8;
            px[1] = (g * (1.0 - strength) + sepia_g * strength).round() as u8;
            px[2] = (b * (1.0 - strength) + sepia_b * strength).round() as u8;
        }
    }

    if params.posterize_levels >= 2 {
        let levels = params.posterize_levels.clamp(2, 64) as f32;
        let step = 255.0 / (levels - 1.0);
        for px in rgba.pixels_mut() {
            px[0] = ((px[0] as f32 / step).round() * step).clamp(0.0, 255.0) as u8;
            px[1] = ((px[1] as f32 / step).round() * step).clamp(0.0, 255.0) as u8;
            px[2] = ((px[2] as f32 / step).round() * step).clamp(0.0, 255.0) as u8;
        }
    }

    if params.vignette_strength > 0.001 {
        apply_vignette_in_place(&mut rgba, params.vignette_strength.clamp(0.0, 1.0));
    }
    if params.emboss_strength > 0.001 {
        apply_emboss_in_place(&mut rgba, params.emboss_strength.clamp(0.0, 1.0));
    }
    if params.edge_enhance_strength > 0.001 {
        apply_edge_enhance_in_place(&mut rgba, params.edge_enhance_strength.clamp(0.0, 1.0));
    }
    if params.stained_glass_strength > 0.001 {
        apply_stained_glass_in_place(&mut rgba, params.stained_glass_strength.clamp(0.0, 1.0));
    }
    if params.tilt_shift_strength > 0.001 {
        apply_tilt_shift_in_place(&mut rgba, params.tilt_shift_strength.clamp(0.0, 1.0));
    }
    if params.oil_paint_strength > 0.001 {
        apply_oil_paint_in_place(&mut rgba, params.oil_paint_strength.clamp(0.0, 1.0));
    }

    DynamicImage::ImageRgba8(rgba)
}

pub(super) fn apply_vignette_in_place(image: &mut RgbaImage, strength: f32) {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let cx = (w as f32 - 1.0) * 0.5;
    let cy = (h as f32 - 1.0) * 0.5;
    let max_dist = (cx * cx + cy * cy).sqrt().max(1.0);
    let keep = 1.0 - strength * 0.85;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt() / max_dist;
            let edge = dist.powf(1.6);
            let factor = (keep + (1.0 - keep) * (1.0 - edge)).clamp(0.0, 1.0);
            let px = image.get_pixel_mut(x, y);
            px[0] = (px[0] as f32 * factor).round().clamp(0.0, 255.0) as u8;
            px[1] = (px[1] as f32 * factor).round().clamp(0.0, 255.0) as u8;
            px[2] = (px[2] as f32 * factor).round().clamp(0.0, 255.0) as u8;
        }
    }
}

pub(super) fn apply_stained_glass_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let block_size = ((4.0 + strength * 28.0).round() as u32).clamp(2, 48);
    for by in (0..h).step_by(block_size as usize) {
        for bx in (0..w).step_by(block_size as usize) {
            let max_x = (bx + block_size).min(w);
            let max_y = (by + block_size).min(h);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for y in by..max_y {
                for x in bx..max_x {
                    let px = source.get_pixel(x, y);
                    sum[0] += px[0] as u64;
                    sum[1] += px[1] as u64;
                    sum[2] += px[2] as u64;
                    sum[3] += px[3] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let avg = [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ];
            for y in by..max_y {
                for x in bx..max_x {
                    let edge = x == bx || y == by || x + 1 == max_x || y + 1 == max_y;
                    if edge {
                        image.put_pixel(
                            x,
                            y,
                            Rgba([
                                ((avg[0] as f32 * 0.35).round() as u8),
                                ((avg[1] as f32 * 0.35).round() as u8),
                                ((avg[2] as f32 * 0.35).round() as u8),
                                avg[3],
                            ]),
                        );
                    } else {
                        image.put_pixel(x, y, Rgba(avg));
                    }
                }
            }
        }
    }
}

pub(super) fn apply_tilt_shift_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let blurred = DynamicImage::ImageRgba8(source.clone())
        .blur((2.5 + strength * 10.0).clamp(0.0, 20.0))
        .to_rgba8();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let center_y = (h as f32 - 1.0) * 0.5;
    let focus_half_band = (h as f32 * (0.09 + (1.0 - strength) * 0.20)).max(6.0);
    let transition = (h as f32 * (0.10 + strength * 0.22)).max(8.0);

    for y in 0..h {
        let dist = (y as f32 - center_y).abs();
        let blur_mix = if dist <= focus_half_band {
            0.0
        } else {
            ((dist - focus_half_band) / transition).clamp(0.0, 1.0)
        };
        for x in 0..w {
            let src = source.get_pixel(x, y);
            let blur = blurred.get_pixel(x, y);
            image.put_pixel(x, y, mix_rgba(*src, *blur, blur_mix));
        }
    }
}

pub(super) fn apply_emboss_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w < 3 || h < 3 {
        return;
    }
    let kernel = [[-2.0f32, -1.0, 0.0], [-1.0, 1.0, 1.0], [0.0, 1.0, 2.0]];
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let mut channel = [0.0f32; 3];
            for ky in 0..3 {
                for kx in 0..3 {
                    let weight = kernel[ky as usize][kx as usize];
                    let px = source.get_pixel(x + kx - 1, y + ky - 1);
                    channel[0] += px[0] as f32 * weight;
                    channel[1] += px[1] as f32 * weight;
                    channel[2] += px[2] as f32 * weight;
                }
            }
            let orig = source.get_pixel(x, y);
            let embossed = [
                (channel[0] + 128.0).clamp(0.0, 255.0) as u8,
                (channel[1] + 128.0).clamp(0.0, 255.0) as u8,
                (channel[2] + 128.0).clamp(0.0, 255.0) as u8,
                orig[3],
            ];
            image.put_pixel(
                x,
                y,
                mix_rgba(*orig, Rgba(embossed), (strength * 0.95).clamp(0.0, 1.0)),
            );
        }
    }
}

pub(super) fn apply_edge_enhance_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w < 3 || h < 3 {
        return;
    }
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let center = source.get_pixel(x, y);
            let mut neighbor_sum = [0i32; 3];
            let mut neighbor_count = 0i32;
            for ny in (y - 1)..=(y + 1) {
                for nx in (x - 1)..=(x + 1) {
                    if nx == x && ny == y {
                        continue;
                    }
                    let px = source.get_pixel(nx, ny);
                    neighbor_sum[0] += px[0] as i32;
                    neighbor_sum[1] += px[1] as i32;
                    neighbor_sum[2] += px[2] as i32;
                    neighbor_count += 1;
                }
            }
            let mut out = [0u8; 4];
            for channel in 0..3 {
                let avg = neighbor_sum[channel] as f32 / neighbor_count as f32;
                let edge = (center[channel] as f32 - avg).abs();
                let boosted = center[channel] as f32 + edge * (strength * 1.6);
                out[channel] = boosted.clamp(0.0, 255.0) as u8;
            }
            out[3] = center[3];
            image.put_pixel(x, y, Rgba(out));
        }
    }
}

pub(super) fn apply_oil_paint_in_place(image: &mut RgbaImage, strength: f32) {
    let source = image.clone();
    let (w, h) = source.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let radius = ((1.0 + strength * 6.0).round() as i32).clamp(1, 8);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            let sample_points = [
                (x as i32, y as i32),
                (x as i32 - radius, y as i32),
                (x as i32 + radius, y as i32),
                (x as i32, y as i32 - radius),
                (x as i32, y as i32 + radius),
                (x as i32 - radius, y as i32 - radius),
                (x as i32 + radius, y as i32 + radius),
            ];
            for (sx, sy) in sample_points {
                if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                    continue;
                }
                let px = source.get_pixel(sx as u32, sy as u32);
                sum[0] += px[0] as u32;
                sum[1] += px[1] as u32;
                sum[2] += px[2] as u32;
                sum[3] += px[3] as u32;
                count += 1;
            }
            if count == 0 {
                continue;
            }
            let avg = Rgba([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ]);
            let original = source.get_pixel(x, y);
            image.put_pixel(x, y, mix_rgba(*original, avg, strength.clamp(0.0, 1.0)));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_perspective_correct(
    image: &DynamicImage,
    top_left: [f32; 2],
    top_right: [f32; 2],
    bottom_right: [f32; 2],
    bottom_left: [f32; 2],
    output_width: u32,
    output_height: u32,
    interpolation: RotationInterpolation,
    fill: [u8; 4],
) -> Result<DynamicImage> {
    if output_width == 0 || output_height == 0 {
        return Err(anyhow!(
            "perspective output dimensions must be greater than zero"
        ));
    }
    let source = image.to_rgba8();
    let mut output = RgbaImage::from_pixel(output_width, output_height, Rgba(fill));
    let denom_x = output_width.saturating_sub(1).max(1) as f32;
    let denom_y = output_height.saturating_sub(1).max(1) as f32;

    for y in 0..output_height {
        let v = y as f32 / denom_y;
        for x in 0..output_width {
            let u = x as f32 / denom_x;
            let source_x = bilinear_quad_value(
                top_left[0],
                top_right[0],
                bottom_right[0],
                bottom_left[0],
                u,
                v,
            );
            let source_y = bilinear_quad_value(
                top_left[1],
                top_right[1],
                bottom_right[1],
                bottom_left[1],
                u,
                v,
            );
            let sampled = match interpolation {
                RotationInterpolation::Nearest => {
                    sample_nearest(&source, source_x, source_y, Rgba(fill))
                }
                RotationInterpolation::Bilinear => {
                    sample_bilinear(&source, source_x, source_y, Rgba(fill))
                }
            };
            output.put_pixel(x, y, sampled);
        }
    }

    Ok(DynamicImage::ImageRgba8(output))
}

pub(super) fn bilinear_quad_value(tl: f32, tr: f32, br: f32, bl: f32, u: f32, v: f32) -> f32 {
    tl * (1.0 - u) * (1.0 - v) + tr * u * (1.0 - v) + br * u * v + bl * (1.0 - u) * v
}

pub(super) fn draw_line(
    image: &mut RgbaImage,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: u32,
    color: Rgba<u8>,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = dx.abs().max(dy.abs()).max(1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = x0 as f32 + dx as f32 * t;
        let y = y0 as f32 + dy as f32 * t;
        draw_disc(
            image,
            x.round() as i32,
            y.round() as i32,
            (thickness as i32 / 2).max(1),
            color,
        );
    }
}

pub(super) fn draw_rectangle(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let min_y = start_y.min(end_y);
    let max_y = start_y.max(end_y);
    if filled {
        fill_rect_blend(
            image,
            min_x,
            min_y,
            (max_x - min_x + 1).max(0) as u32,
            (max_y - min_y + 1).max(0) as u32,
            color,
        );
    } else {
        let t = thickness.max(1) as i32;
        fill_rect_blend(
            image,
            min_x,
            min_y,
            (max_x - min_x + 1).max(0) as u32,
            t as u32,
            color,
        );
        fill_rect_blend(
            image,
            min_x,
            max_y - t + 1,
            (max_x - min_x + 1).max(0) as u32,
            t as u32,
            color,
        );
        fill_rect_blend(
            image,
            min_x,
            min_y,
            t as u32,
            (max_y - min_y + 1).max(0) as u32,
            color,
        );
        fill_rect_blend(
            image,
            max_x - t + 1,
            min_y,
            t as u32,
            (max_y - min_y + 1).max(0) as u32,
            color,
        );
    }
}

pub(super) fn draw_ellipse(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let min_y = start_y.min(end_y);
    let max_y = start_y.max(end_y);
    let cx = (min_x + max_x) as f32 * 0.5;
    let cy = (min_y + max_y) as f32 * 0.5;
    let rx = ((max_x - min_x).max(1) as f32) * 0.5;
    let ry = ((max_y - min_y).max(1) as f32) * 0.5;
    let stroke = (thickness as f32 / 2.0).max(1.0);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let nx = (x as f32 - cx) / rx;
            let ny = (y as f32 - cy) / ry;
            let d = nx * nx + ny * ny;
            let inside = d <= 1.0;
            let on_edge = (d - 1.0).abs() <= (stroke / rx.max(ry)).max(0.02);
            if (filled && inside) || (!filled && on_edge) {
                blend_pixel_safe(image, x, y, color);
            }
        }
    }
}

pub(super) fn draw_arrow(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    color: Rgba<u8>,
) {
    draw_line(image, start_x, start_y, end_x, end_y, thickness, color);
    let dx = (end_x - start_x) as f32;
    let dy = (end_y - start_y) as f32;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let head_len = (thickness.max(2) as f32 * 4.0).max(8.0);
    let side = (thickness.max(2) as f32 * 2.0).max(5.0);
    let bx = end_x as f32 - ux * head_len;
    let by = end_y as f32 - uy * head_len;
    let px = -uy;
    let py = ux;
    let left_x = (bx + px * side).round() as i32;
    let left_y = (by + py * side).round() as i32;
    let right_x = (bx - px * side).round() as i32;
    let right_y = (by - py * side).round() as i32;
    draw_line(image, end_x, end_y, left_x, left_y, thickness, color);
    draw_line(image, end_x, end_y, right_x, right_y, thickness, color);
}

pub(super) fn draw_rounded_rect_shadow(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let shadow = Rgba([0, 0, 0, 110]);
    let offset = (thickness.max(2) / 2) as i32 + 2;
    draw_rounded_rect(
        image,
        start_x + offset,
        start_y + offset,
        end_x + offset,
        end_y + offset,
        thickness.max(2),
        true,
        shadow,
    );
    draw_rounded_rect(
        image, start_x, start_y, end_x, end_y, thickness, filled, color,
    );
}

pub(super) fn draw_rounded_rect(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let min_y = start_y.min(end_y);
    let max_y = start_y.max(end_y);
    let width = (max_x - min_x + 1).max(1);
    let height = (max_y - min_y + 1).max(1);
    let radius = ((width.min(height) as f32) * 0.16).round() as i32;
    let radius = radius.clamp(2, 48);
    if filled {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if rounded_rect_contains(x, y, min_x, min_y, max_x, max_y, radius) {
                    blend_pixel_safe(image, x, y, color);
                }
            }
        }
    } else {
        let t = thickness.max(1) as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let inside = rounded_rect_contains(x, y, min_x, min_y, max_x, max_y, radius);
                if !inside {
                    continue;
                }
                let inner = rounded_rect_contains(
                    x,
                    y,
                    min_x + t,
                    min_y + t,
                    max_x - t,
                    max_y - t,
                    (radius - t).max(0),
                );
                if !inner {
                    blend_pixel_safe(image, x, y, color);
                }
            }
        }
    }
}

pub(super) fn rounded_rect_contains(
    x: i32,
    y: i32,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
    radius: i32,
) -> bool {
    if x < min_x || x > max_x || y < min_y || y > max_y {
        return false;
    }
    if radius <= 0 {
        return true;
    }
    let corner_x = if x < min_x + radius {
        min_x + radius
    } else if x > max_x - radius {
        max_x - radius
    } else {
        x
    };
    let corner_y = if y < min_y + radius {
        min_y + radius
    } else if y > max_y - radius {
        max_y - radius
    } else {
        y
    };
    let dx = x - corner_x;
    let dy = y - corner_y;
    dx * dx + dy * dy <= radius * radius
}

pub(super) fn draw_speech_bubble(
    image: &mut RgbaImage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: u32,
    filled: bool,
    color: Rgba<u8>,
) {
    draw_rounded_rect(
        image, start_x, start_y, end_x, end_y, thickness, filled, color,
    );
    let min_x = start_x.min(end_x);
    let max_x = start_x.max(end_x);
    let max_y = start_y.max(end_y);
    let tail_w = ((max_x - min_x).abs() as f32 * 0.16).round() as i32;
    let tail_w = tail_w.clamp(10, 72);
    let tail_h = (tail_w as f32 * 0.8).round() as i32;
    let tip_x = min_x + ((max_x - min_x) as f32 * 0.25).round() as i32;
    let tip_y = max_y + tail_h;
    let left_x = tip_x - tail_w / 2;
    let left_y = max_y - 1;
    let right_x = tip_x + tail_w / 2;
    let right_y = max_y - 1;
    draw_line(image, left_x, left_y, tip_x, tip_y, thickness, color);
    draw_line(image, tip_x, tip_y, right_x, right_y, thickness, color);
    if filled {
        for y in max_y..=tip_y {
            let t = (y - max_y) as f32 / (tail_h.max(1) as f32);
            let row_left = (left_x as f32 + (tip_x - left_x) as f32 * t).round() as i32;
            let row_right = (right_x as f32 + (tip_x - right_x) as f32 * t).round() as i32;
            for x in row_left.min(row_right)..=row_left.max(row_right) {
                blend_pixel_safe(image, x, y, color);
            }
        }
    }
}

pub(super) fn draw_disc(image: &mut RgbaImage, cx: i32, cy: i32, radius: i32, color: Rgba<u8>) {
    let radius = radius.max(1);
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= radius * radius {
                blend_pixel_safe(image, cx + x, cy + y, color);
            }
        }
    }
}

pub(super) fn fill_rect_blend(
    image: &mut RgbaImage,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: Rgba<u8>,
) {
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        for col in 0..width {
            blend_pixel_safe(image, x + col as i32, y + row as i32, color);
        }
    }
}

pub(super) fn blend_pixel_safe(image: &mut RgbaImage, x: i32, y: i32, src: Rgba<u8>) {
    if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
        return;
    }
    let dst = image.get_pixel_mut(x as u32, y as u32);
    blend_pixel(dst, src);
}

pub(super) fn blend_pixel(dst: &mut Rgba<u8>, src: Rgba<u8>) {
    let src_a = src[3] as f32 / 255.0;
    if src_a <= 0.0 {
        return;
    }
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return;
    }
    for channel in 0..3 {
        let src_v = src[channel] as f32 / 255.0;
        let dst_v = dst[channel] as f32 / 255.0;
        let out_v = (src_v * src_a + dst_v * dst_a * (1.0 - src_a)) / out_a;
        dst[channel] = (out_v * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

pub(super) fn apply_border(
    image: &DynamicImage,
    left: u32,
    right: u32,
    top: u32,
    bottom: u32,
    color: [u8; 4],
) -> Result<DynamicImage> {
    if left == 0 && right == 0 && top == 0 && bottom == 0 {
        return Ok(image.clone());
    }

    let source = image.to_rgba8();
    let (source_width, source_height) = source.dimensions();
    let width = source_width
        .checked_add(left)
        .and_then(|v| v.checked_add(right))
        .context("border width overflows image dimensions")?;
    let height = source_height
        .checked_add(top)
        .and_then(|v| v.checked_add(bottom))
        .context("border height overflows image dimensions")?;

    let mut output = RgbaImage::from_pixel(width, height, Rgba(color));
    for y in 0..source_height {
        for x in 0..source_width {
            output.put_pixel(x + left, y + top, *source.get_pixel(x, y));
        }
    }
    Ok(DynamicImage::ImageRgba8(output))
}

pub(super) fn apply_canvas_size(
    image: &DynamicImage,
    width: u32,
    height: u32,
    anchor: CanvasAnchor,
    fill: [u8; 4],
) -> Result<DynamicImage> {
    if width == 0 || height == 0 {
        return Err(anyhow!("canvas dimensions must be greater than zero"));
    }

    let source = image.to_rgba8();
    let (source_width, source_height) = source.dimensions();
    if width == source_width && height == source_height {
        return Ok(DynamicImage::ImageRgba8(source));
    }

    let (factor_x, factor_y) = anchor.factors();
    let (src_x, dst_x, copy_width) = axis_mapping(source_width, width, factor_x);
    let (src_y, dst_y, copy_height) = axis_mapping(source_height, height, factor_y);

    let mut output = RgbaImage::from_pixel(width, height, Rgba(fill));
    for y in 0..copy_height {
        for x in 0..copy_width {
            output.put_pixel(
                dst_x + x,
                dst_y + y,
                *source.get_pixel(src_x + x, src_y + y),
            );
        }
    }
    Ok(DynamicImage::ImageRgba8(output))
}

pub(super) fn axis_mapping(source: u32, target: u32, factor: f32) -> (u32, u32, u32) {
    let copy_len = source.min(target);
    if target >= source {
        let pad = target - source;
        let dst = ((pad as f32 * factor).round() as u32).min(pad);
        (0, dst, copy_len)
    } else {
        let trim = source - target;
        let src = ((trim as f32 * factor).round() as u32).min(trim);
        (src, 0, copy_len)
    }
}

pub(super) fn apply_rotate_fine(
    image: &DynamicImage,
    angle_degrees: f32,
    interpolation: RotationInterpolation,
    expand_canvas: bool,
    fill: [u8; 4],
) -> Result<DynamicImage> {
    if !angle_degrees.is_finite() {
        return Err(anyhow!("rotation angle must be finite"));
    }

    let source = image.to_rgba8();
    let (source_width, source_height) = source.dimensions();
    if source_width == 0 || source_height == 0 {
        return Err(anyhow!("cannot rotate empty image"));
    }

    if angle_degrees.abs() < f32::EPSILON {
        return Ok(DynamicImage::ImageRgba8(source));
    }

    let radians = angle_degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let (target_width, target_height) = if expand_canvas {
        let abs_cos = cos.abs();
        let abs_sin = sin.abs();
        (
            ((source_width as f32 * abs_cos + source_height as f32 * abs_sin)
                .ceil()
                .max(1.0)) as u32,
            ((source_width as f32 * abs_sin + source_height as f32 * abs_cos)
                .ceil()
                .max(1.0)) as u32,
        )
    } else {
        (source_width, source_height)
    };

    let source_cx = (source_width as f32 - 1.0) * 0.5;
    let source_cy = (source_height as f32 - 1.0) * 0.5;
    let target_cx = (target_width as f32 - 1.0) * 0.5;
    let target_cy = (target_height as f32 - 1.0) * 0.5;

    let mut output = RgbaImage::from_pixel(target_width, target_height, Rgba(fill));
    for y in 0..target_height {
        let dy = y as f32 - target_cy;
        for x in 0..target_width {
            let dx = x as f32 - target_cx;
            let source_x = cos * dx + sin * dy + source_cx;
            let source_y = -sin * dx + cos * dy + source_cy;

            let sampled = match interpolation {
                RotationInterpolation::Nearest => {
                    sample_nearest(&source, source_x, source_y, Rgba(fill))
                }
                RotationInterpolation::Bilinear => {
                    sample_bilinear(&source, source_x, source_y, Rgba(fill))
                }
            };
            output.put_pixel(x, y, sampled);
        }
    }

    Ok(DynamicImage::ImageRgba8(output))
}

pub(super) fn sample_nearest(
    source: &RgbaImage,
    source_x: f32,
    source_y: f32,
    fallback: Rgba<u8>,
) -> Rgba<u8> {
    let x = source_x.round() as i32;
    let y = source_y.round() as i32;
    if x < 0 || y < 0 || x >= source.width() as i32 || y >= source.height() as i32 {
        return fallback;
    }
    *source.get_pixel(x as u32, y as u32)
}

pub(super) fn sample_bilinear(
    source: &RgbaImage,
    source_x: f32,
    source_y: f32,
    fallback: Rgba<u8>,
) -> Rgba<u8> {
    if source_x < 0.0
        || source_y < 0.0
        || source_x > (source.width() - 1) as f32
        || source_y > (source.height() - 1) as f32
    {
        return fallback;
    }

    let x0 = source_x.floor() as u32;
    let y0 = source_y.floor() as u32;
    let x1 = (x0 + 1).min(source.width() - 1);
    let y1 = (y0 + 1).min(source.height() - 1);

    let tx = source_x - x0 as f32;
    let ty = source_y - y0 as f32;
    let p00 = source.get_pixel(x0, y0).0;
    let p10 = source.get_pixel(x1, y0).0;
    let p01 = source.get_pixel(x0, y1).0;
    let p11 = source.get_pixel(x1, y1).0;

    let mut output = [0u8; 4];
    for channel in 0..4 {
        let top = p00[channel] as f32 * (1.0 - tx) + p10[channel] as f32 * tx;
        let bottom = p01[channel] as f32 * (1.0 - tx) + p11[channel] as f32 * tx;
        let value = top * (1.0 - ty) + bottom * ty;
        output[channel] = value.round().clamp(0.0, 255.0) as u8;
    }

    Rgba(output)
}

pub(super) fn apply_color_adjustments(image: &DynamicImage, params: ColorAdjustParams) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    let brightness = params.brightness.clamp(-255, 255) as f32 / 255.0;
    let contrast = 1.0 + (params.contrast.clamp(-100.0, 100.0) / 100.0);
    let gamma = params.gamma.clamp(0.1, 5.0);
    let saturation = params.saturation.clamp(0.0, 3.0);

    for pixel in rgba.pixels_mut() {
        let alpha = pixel[3];
        let mut r = pixel[0] as f32 / 255.0;
        let mut g = pixel[1] as f32 / 255.0;
        let mut b = pixel[2] as f32 / 255.0;

        r = ((r + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
        g = ((g + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
        b = ((b + brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);

        let gray = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
        r = (gray + (r - gray) * saturation).clamp(0.0, 1.0);
        g = (gray + (g - gray) * saturation).clamp(0.0, 1.0);
        b = (gray + (b - gray) * saturation).clamp(0.0, 1.0);

        if params.grayscale {
            r = gray;
            g = gray;
            b = gray;
        }

        r = r.powf(1.0 / gamma).clamp(0.0, 1.0);
        g = g.powf(1.0 / gamma).clamp(0.0, 1.0);
        b = b.powf(1.0 / gamma).clamp(0.0, 1.0);

        pixel[0] = (r * 255.0).round() as u8;
        pixel[1] = (g * 255.0).round() as u8;
        pixel[2] = (b * 255.0).round() as u8;
        pixel[3] = alpha;
    }

    DynamicImage::ImageRgba8(rgba)
}
