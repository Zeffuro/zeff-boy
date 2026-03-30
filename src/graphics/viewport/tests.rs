use super::*;

#[test]
fn stretch_fills_available_area() {
    let vp = calculate_viewport(AspectRatioMode::Stretch, 800, 600, 160, 144, 0.0);
    assert_eq!(vp, Some((0.0, 0.0, 800.0, 600.0)));
}

#[test]
fn stretch_with_top_offset() {
    let vp = calculate_viewport(AspectRatioMode::Stretch, 800, 600, 160, 144, 30.0);
    assert_eq!(vp, Some((0.0, 30.0, 800.0, 570.0)));
}

#[test]
fn keep_aspect_centers_horizontally() {
    let vp = calculate_viewport(AspectRatioMode::KeepAspect, 800, 600, 160, 144, 0.0);
    let (x, y, w, h) = vp.unwrap();
    assert!(w <= 800.0);
    assert!(h <= 600.0);
    assert!(x >= 0.0);
    assert!(y >= 0.0);
}

#[test]
fn integer_scale_gb_in_800x600() {
    let vp = calculate_viewport(AspectRatioMode::IntegerScale, 800, 600, 160, 144, 0.0);
    let (x, y, w, h) = vp.unwrap();
    assert_eq!(w, 640.0);
    assert_eq!(h, 576.0);
    assert_eq!(x, 80.0);
    assert_eq!(y, 12.0);
}

#[test]
fn integer_scale_nes_in_1920x1080() {
    let vp = calculate_viewport(AspectRatioMode::IntegerScale, 1920, 1080, 256, 240, 0.0);
    let (x, y, w, h) = vp.unwrap();
    assert_eq!(w, 1024.0);
    assert_eq!(h, 960.0);
    assert_eq!(x, 448.0);
    assert_eq!(y, 60.0);
}

#[test]
fn returns_none_for_zero_window() {
    assert!(calculate_viewport(AspectRatioMode::Stretch, 0, 0, 160, 144, 0.0).is_none());
}

#[test]
fn returns_none_when_offset_fills_window() {
    assert!(calculate_viewport(AspectRatioMode::KeepAspect, 800, 30, 160, 144, 30.0).is_none());
}

#[test]
fn integer_scale_minimum_1x() {
    let vp = calculate_viewport(AspectRatioMode::IntegerScale, 100, 100, 160, 144, 0.0);
    let (_, _, w, h) = vp.unwrap();
    assert_eq!(w, 160.0);
    assert_eq!(h, 144.0);
}

