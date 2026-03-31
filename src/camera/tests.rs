use super::*;

#[test]
fn checkerboard_frame_has_expected_size() {
    assert_eq!(checkerboard_frame().len(), FRAME_LEN);
}

#[test]
fn rgb_nearest_resize_outputs_expected_size() {
    let src = vec![255u8; 4 * 4 * 3];
    let out = rgb_to_grayscale_nearest(&src, 4, 4, CAMERA_WIDTH, CAMERA_HEIGHT);
    assert_eq!(out.len(), FRAME_LEN);
}
