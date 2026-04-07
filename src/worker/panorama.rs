use super::*;

pub(super) fn stitch_images(
    images: &[RgbaImage],
    direction: PanoramaDirection,
    overlap_percent: f32,
) -> RgbaImage {
    let mut iter = images.iter();
    let Some(first) = iter.next() else {
        return RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
    };
    let mut canvas = first.clone();

    for image in iter {
        match direction {
            PanoramaDirection::Horizontal => {
                let overlap =
                    ((canvas.width().min(image.width()) as f32) * overlap_percent).round() as u32;
                let overlap = overlap.min(canvas.width().min(image.width()).saturating_sub(1));
                let shift_y = estimate_vertical_overlap_shift(&canvas, image, overlap);
                let existing_origin_y = if shift_y < 0 { (-shift_y) as u32 } else { 0 };
                let incoming_origin_y = if shift_y > 0 { shift_y as u32 } else { 0 };
                let incoming_origin_x = canvas.width() - overlap;
                let mut next = RgbaImage::from_pixel(
                    canvas.width() + image.width() - overlap,
                    (existing_origin_y + canvas.height()).max(incoming_origin_y + image.height()),
                    Rgba([0, 0, 0, 255]),
                );
                blit_rgba(&canvas, &mut next, 0, existing_origin_y);
                blend_with_vertical_seam(
                    &canvas,
                    (0, existing_origin_y),
                    image,
                    (incoming_origin_x, incoming_origin_y),
                    &mut next,
                    overlap,
                );
                canvas = next;
            }
            PanoramaDirection::Vertical => {
                let overlap =
                    ((canvas.height().min(image.height()) as f32) * overlap_percent).round() as u32;
                let overlap = overlap.min(canvas.height().min(image.height()).saturating_sub(1));
                let shift_x = estimate_horizontal_overlap_shift(&canvas, image, overlap);
                let existing_origin_x = if shift_x < 0 { (-shift_x) as u32 } else { 0 };
                let incoming_origin_x = if shift_x > 0 { shift_x as u32 } else { 0 };
                let incoming_origin_y = canvas.height() - overlap;
                let mut next = RgbaImage::from_pixel(
                    (existing_origin_x + canvas.width()).max(incoming_origin_x + image.width()),
                    canvas.height() + image.height() - overlap,
                    Rgba([0, 0, 0, 255]),
                );
                blit_rgba(&canvas, &mut next, existing_origin_x, 0);
                blend_with_horizontal_seam(
                    &canvas,
                    (existing_origin_x, 0),
                    image,
                    (incoming_origin_x, incoming_origin_y),
                    &mut next,
                    overlap,
                );
                canvas = next;
            }
        }
    }
    canvas
}

pub(super) fn blend_with_vertical_seam(
    existing: &RgbaImage,
    existing_origin: (u32, u32),
    incoming: &RgbaImage,
    incoming_origin: (u32, u32),
    out: &mut RgbaImage,
    overlap: u32,
) {
    if overlap == 0 {
        blit_rgba(incoming, out, incoming_origin.0, incoming_origin.1);
        return;
    }

    let overlap_left = incoming_origin.0;
    let overlap_right = incoming_origin.0.saturating_add(overlap);
    let left = overlap_left
        .max(existing_origin.0)
        .max(incoming_origin.0)
        .min(out.width());
    let right = overlap_right
        .min(existing_origin.0.saturating_add(existing.width()))
        .min(incoming_origin.0.saturating_add(incoming.width()))
        .min(out.width());
    let top = existing_origin.1.max(incoming_origin.1).min(out.height());
    let bottom = existing_origin
        .1
        .saturating_add(existing.height())
        .min(incoming_origin.1.saturating_add(incoming.height()))
        .min(out.height());

    let overlap_w = right.saturating_sub(left);
    let overlap_h = bottom.saturating_sub(top);
    if overlap_w == 0 || overlap_h == 0 {
        blit_rgba(incoming, out, incoming_origin.0, incoming_origin.1);
        return;
    }

    let mut cost = vec![vec![0u32; overlap_w as usize]; overlap_h as usize];
    for y in 0..overlap_h {
        for x in 0..overlap_w {
            let out_x = left + x;
            let out_y = top + y;
            let existing_px = existing.get_pixel(
                out_x.saturating_sub(existing_origin.0),
                out_y.saturating_sub(existing_origin.1),
            );
            let incoming_px = incoming.get_pixel(
                out_x.saturating_sub(incoming_origin.0),
                out_y.saturating_sub(incoming_origin.1),
            );
            cost[y as usize][x as usize] = rgb_distance_sq(*existing_px, *incoming_px);
        }
    }
    let seam = compute_vertical_seam(&cost);
    let blend_band = ((overlap_w as i32) / 24).clamp(1, 6);

    for y in 0..incoming.height() {
        for x in 0..incoming.width() {
            let dx = incoming_origin.0 + x;
            let dy = incoming_origin.1 + y;
            if dx >= out.width() || dy >= out.height() {
                continue;
            }

            let src_px = *incoming.get_pixel(x, y);
            if dx < left || dx >= right || dy < top || dy >= bottom {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let local_x = dx.saturating_sub(left) as i32;
            let local_y = dy.saturating_sub(top) as usize;
            let seam_x = seam[local_y] as i32;
            if local_x < seam_x - blend_band {
                continue;
            }
            if local_x > seam_x + blend_band {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let blend_t = if blend_band <= 0 {
                0.5
            } else {
                let left = seam_x - blend_band;
                ((local_x - left) as f32 / (blend_band * 2) as f32).clamp(0.0, 1.0)
            };
            let dst_px = *out.get_pixel(dx, dy);
            out.put_pixel(dx, dy, mix_rgba(dst_px, src_px, blend_t));
        }
    }
}

pub(super) fn blend_with_horizontal_seam(
    existing: &RgbaImage,
    existing_origin: (u32, u32),
    incoming: &RgbaImage,
    incoming_origin: (u32, u32),
    out: &mut RgbaImage,
    overlap: u32,
) {
    if overlap == 0 {
        blit_rgba(incoming, out, incoming_origin.0, incoming_origin.1);
        return;
    }

    let overlap_top = incoming_origin.1;
    let overlap_bottom = incoming_origin.1.saturating_add(overlap);
    let left = existing_origin.0.max(incoming_origin.0).min(out.width());
    let right = existing_origin
        .0
        .saturating_add(existing.width())
        .min(incoming_origin.0.saturating_add(incoming.width()))
        .min(out.width());
    let top = overlap_top
        .max(existing_origin.1)
        .max(incoming_origin.1)
        .min(out.height());
    let bottom = overlap_bottom
        .min(existing_origin.1.saturating_add(existing.height()))
        .min(incoming_origin.1.saturating_add(incoming.height()))
        .min(out.height());

    let overlap_h = bottom.saturating_sub(top);
    let overlap_w = right.saturating_sub(left);
    if overlap_w == 0 || overlap_h == 0 {
        blit_rgba(incoming, out, incoming_origin.0, incoming_origin.1);
        return;
    }

    let mut cost = vec![vec![0u32; overlap_h as usize]; overlap_w as usize];
    for x in 0..overlap_w {
        for y in 0..overlap_h {
            let out_x = left + x;
            let out_y = top + y;
            let existing_px = existing.get_pixel(
                out_x.saturating_sub(existing_origin.0),
                out_y.saturating_sub(existing_origin.1),
            );
            let incoming_px = incoming.get_pixel(
                out_x.saturating_sub(incoming_origin.0),
                out_y.saturating_sub(incoming_origin.1),
            );
            cost[x as usize][y as usize] = rgb_distance_sq(*existing_px, *incoming_px);
        }
    }
    let seam = compute_horizontal_seam(&cost);
    let blend_band = ((overlap_h as i32) / 24).clamp(1, 6);

    for y in 0..incoming.height() {
        for x in 0..incoming.width() {
            let dx = incoming_origin.0 + x;
            let dy = incoming_origin.1 + y;
            if dx >= out.width() || dy >= out.height() {
                continue;
            }

            let src_px = *incoming.get_pixel(x, y);
            if dx < left || dx >= right || dy < top || dy >= bottom {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let local_x = dx.saturating_sub(left) as usize;
            let local_y = dy.saturating_sub(top) as i32;
            let seam_y = seam[local_x] as i32;
            if local_y < seam_y - blend_band {
                continue;
            }
            if local_y > seam_y + blend_band {
                out.put_pixel(dx, dy, src_px);
                continue;
            }

            let blend_t = if blend_band <= 0 {
                0.5
            } else {
                let top = seam_y - blend_band;
                ((local_y - top) as f32 / (blend_band * 2) as f32).clamp(0.0, 1.0)
            };
            let dst_px = *out.get_pixel(dx, dy);
            out.put_pixel(dx, dy, mix_rgba(dst_px, src_px, blend_t));
        }
    }
}

pub(super) fn estimate_vertical_overlap_shift(
    existing: &RgbaImage,
    incoming: &RgbaImage,
    overlap: u32,
) -> i32 {
    let overlap_w = overlap.min(existing.width()).min(incoming.width());
    if overlap_w == 0 {
        return 0;
    }
    let max_shift = ((existing.height().min(incoming.height()) as f32) * 0.2)
        .round()
        .clamp(0.0, 96.0) as i32;
    if max_shift == 0 {
        return 0;
    }

    let x_step = (overlap_w / 40).max(1);
    let y_step = (existing.height().min(incoming.height()) / 40).max(1);
    let existing_x_start = existing.width().saturating_sub(overlap_w);

    let mut best_shift = 0;
    let mut best_score = u128::MAX;
    for shift in -max_shift..=max_shift {
        let mut score = 0u128;
        let mut samples = 0u32;
        let mut y = 0u32;
        while y < incoming.height() {
            let existing_y = y as i32 + shift;
            if existing_y >= 0 && existing_y < existing.height() as i32 {
                let existing_y = existing_y as u32;
                let mut x = 0u32;
                while x < overlap_w {
                    let existing_px = existing.get_pixel(existing_x_start + x, existing_y);
                    let incoming_px = incoming.get_pixel(x, y);
                    score =
                        score.saturating_add(rgb_distance_sq(*existing_px, *incoming_px) as u128);
                    samples = samples.saturating_add(1);
                    x = x.saturating_add(x_step);
                }
            }
            y = y.saturating_add(y_step);
        }
        if samples == 0 {
            continue;
        }
        let avg = score / samples as u128;
        let penalized = avg.saturating_add((shift.unsigned_abs() as u128).saturating_mul(3));
        if penalized < best_score {
            best_score = penalized;
            best_shift = shift;
        }
    }

    best_shift
}

pub(super) fn estimate_horizontal_overlap_shift(
    existing: &RgbaImage,
    incoming: &RgbaImage,
    overlap: u32,
) -> i32 {
    let overlap_h = overlap.min(existing.height()).min(incoming.height());
    if overlap_h == 0 {
        return 0;
    }
    let max_shift = ((existing.width().min(incoming.width()) as f32) * 0.2)
        .round()
        .clamp(0.0, 96.0) as i32;
    if max_shift == 0 {
        return 0;
    }

    let x_step = (existing.width().min(incoming.width()) / 40).max(1);
    let y_step = (overlap_h / 40).max(1);
    let existing_y_start = existing.height().saturating_sub(overlap_h);

    let mut best_shift = 0;
    let mut best_score = u128::MAX;
    for shift in -max_shift..=max_shift {
        let mut score = 0u128;
        let mut samples = 0u32;
        let mut y = 0u32;
        while y < overlap_h {
            let mut x = 0u32;
            while x < incoming.width() {
                let existing_x = x as i32 + shift;
                if existing_x >= 0 && existing_x < existing.width() as i32 {
                    let existing_px = existing.get_pixel(existing_x as u32, existing_y_start + y);
                    let incoming_px = incoming.get_pixel(x, y);
                    score =
                        score.saturating_add(rgb_distance_sq(*existing_px, *incoming_px) as u128);
                    samples = samples.saturating_add(1);
                }
                x = x.saturating_add(x_step);
            }
            y = y.saturating_add(y_step);
        }
        if samples == 0 {
            continue;
        }
        let avg = score / samples as u128;
        let penalized = avg.saturating_add((shift.unsigned_abs() as u128).saturating_mul(3));
        if penalized < best_score {
            best_score = penalized;
            best_shift = shift;
        }
    }

    best_shift
}

pub(super) fn compute_vertical_seam(cost: &[Vec<u32>]) -> Vec<usize> {
    let rows = cost.len();
    let cols = cost.first().map(|row| row.len()).unwrap_or(0);
    if rows == 0 || cols == 0 {
        return Vec::new();
    }

    let mut dp = vec![vec![0u64; cols]; rows];
    let mut parent = vec![vec![0usize; cols]; rows];
    for x in 0..cols {
        dp[0][x] = cost[0][x] as u64;
        parent[0][x] = x;
    }

    for y in 1..rows {
        for x in 0..cols {
            let mut best_prev = x;
            let mut best_cost = dp[y - 1][x];
            if x > 0 && dp[y - 1][x - 1] < best_cost {
                best_cost = dp[y - 1][x - 1];
                best_prev = x - 1;
            }
            if x + 1 < cols && dp[y - 1][x + 1] < best_cost {
                best_cost = dp[y - 1][x + 1];
                best_prev = x + 1;
            }
            dp[y][x] = best_cost + cost[y][x] as u64;
            parent[y][x] = best_prev;
        }
    }

    let mut end_x = 0usize;
    let mut end_cost = dp[rows - 1][0];
    for x in 1..cols {
        if dp[rows - 1][x] < end_cost {
            end_cost = dp[rows - 1][x];
            end_x = x;
        }
    }

    let mut seam = vec![0usize; rows];
    let mut x = end_x;
    for y in (0..rows).rev() {
        seam[y] = x;
        if y > 0 {
            x = parent[y][x];
        }
    }
    seam
}

pub(super) fn compute_horizontal_seam(cost: &[Vec<u32>]) -> Vec<usize> {
    let cols = cost.len();
    let rows = cost.first().map(|column| column.len()).unwrap_or(0);
    if cols == 0 || rows == 0 {
        return Vec::new();
    }

    let mut dp = vec![vec![0u64; rows]; cols];
    let mut parent = vec![vec![0usize; rows]; cols];
    for y in 0..rows {
        dp[0][y] = cost[0][y] as u64;
        parent[0][y] = y;
    }

    for x in 1..cols {
        for y in 0..rows {
            let mut best_prev = y;
            let mut best_cost = dp[x - 1][y];
            if y > 0 && dp[x - 1][y - 1] < best_cost {
                best_cost = dp[x - 1][y - 1];
                best_prev = y - 1;
            }
            if y + 1 < rows && dp[x - 1][y + 1] < best_cost {
                best_cost = dp[x - 1][y + 1];
                best_prev = y + 1;
            }
            dp[x][y] = best_cost + cost[x][y] as u64;
            parent[x][y] = best_prev;
        }
    }

    let mut end_y = 0usize;
    let mut end_cost = dp[cols - 1][0];
    for y in 1..rows {
        if dp[cols - 1][y] < end_cost {
            end_cost = dp[cols - 1][y];
            end_y = y;
        }
    }

    let mut seam = vec![0usize; cols];
    let mut y = end_y;
    for x in (0..cols).rev() {
        seam[x] = y;
        if x > 0 {
            y = parent[x][y];
        }
    }
    seam
}

pub(super) fn rgb_distance_sq(a: Rgba<u8>, b: Rgba<u8>) -> u32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    (dr * dr + dg * dg + db * db) as u32
}

pub(super) fn mix_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    let t = t.clamp(0.0, 1.0);
    let mix = |lhs: u8, rhs: u8| -> u8 {
        (lhs as f32 * (1.0 - t) + rhs as f32 * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba([
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    ])
}

pub(super) fn blit_rgba(src: &RgbaImage, dst: &mut RgbaImage, offset_x: u32, offset_y: u32) {
    for y in 0..src.height() {
        let dy = offset_y + y;
        if dy >= dst.height() {
            break;
        }
        for x in 0..src.width() {
            let dx = offset_x + x;
            if dx >= dst.width() {
                break;
            }
            dst.put_pixel(dx, dy, *src.get_pixel(x, y));
        }
    }
}
