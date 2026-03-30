#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AspectRatioMode {
    Stretch,
    KeepAspect,
    IntegerScale,
}

pub(crate) fn calculate_viewport(
    mode: AspectRatioMode,
    window_width: u32,
    window_height: u32,
    game_width: u32,
    game_height: u32,
    top_offset: f32,
) -> Option<(f32, f32, f32, f32)> {
    let ww = window_width as f32;
    let wh = window_height as f32;
    let available_h = (wh - top_offset).max(0.0);
    if ww <= 0.0 || available_h <= 0.0 {
        return None;
    }

    match mode {
        AspectRatioMode::Stretch => Some((0.0, top_offset, ww, available_h)),
        AspectRatioMode::KeepAspect => {
            let scale_x = ww / game_width as f32;
            let scale_y = available_h / game_height as f32;
            let scale = scale_x.min(scale_y);
            let w = (game_width as f32 * scale).floor();
            let h = (game_height as f32 * scale).floor();
            let x = ((ww - w) / 2.0).floor();
            let y = top_offset + ((available_h - h) / 2.0).floor();
            Some((x, y, w, h))
        }
        AspectRatioMode::IntegerScale => {
            let scale_x = window_width / game_width;
            let visible_h = (available_h.floor() as u32).max(1);
            let scale_y = visible_h / game_height;
            let scale = scale_x.min(scale_y).max(1);
            let w = game_width * scale;
            let h = game_height * scale;
            let x = (window_width.saturating_sub(w)) / 2;
            let y = ((visible_h.saturating_sub(h)) / 2) as f32 + top_offset;
            Some((x as f32, y, w as f32, h as f32))
        }
    }
}

#[cfg(test)]
mod tests;
