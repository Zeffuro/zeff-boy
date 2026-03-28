use super::*;

#[test]
fn encode_2bpp_single_tile_all_white() {
    let pixels = vec![0u8; 64];
    let tiles = encode_2bpp_tiles(&pixels, 8, 8);
    assert_eq!(tiles.len(), 16);
    assert!(tiles.iter().all(|&b| b == 0));
}

#[test]
fn encode_2bpp_single_tile_all_black() {
    // 2-bit value 3 = darkest
    let pixels = vec![3u8; 64];
    let tiles = encode_2bpp_tiles(&pixels, 8, 8);
    assert_eq!(tiles.len(), 16);
    // Each row: lo=0xFF, hi=0xFF
    for row in 0..8 {
        assert_eq!(tiles[row * 2], 0xFF);
        assert_eq!(tiles[row * 2 + 1], 0xFF);
    }
}

#[test]
fn encode_2bpp_correct_tile_count() {
    let pixels = vec![0u8; CAMERA_WIDTH * CAMERA_HEIGHT];
    let tiles = encode_2bpp_tiles(&pixels, CAMERA_WIDTH, CAMERA_HEIGHT);
    assert_eq!(tiles.len(), CAMERA_IMAGE_BYTES);
}

#[test]
fn camera_image_bytes_matches_spec() {
    assert_eq!(CAMERA_IMAGE_BYTES, 16 * 14 * 16);
}

#[test]
fn threshold_triplet_uses_dither_planes() {
    let mut regs = [0u8; 48];
    regs[0] = 40;
    regs[16] = 120;
    regs[32] = 200;
    assert_eq!(threshold_triplet(&regs, 0), (40, 120, 200));
}

#[test]
fn threshold_triplet_falls_back_when_uninitialized() {
    let regs = [0u8; 48];
    assert_eq!(threshold_triplet(&regs, 0), (64, 128, 192));
}

