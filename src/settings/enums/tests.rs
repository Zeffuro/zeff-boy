use super::*;

#[test]
fn vsync_on_always_fifo() {
    let caps = vec![
        wgpu::PresentMode::Fifo,
        wgpu::PresentMode::Immediate,
        wgpu::PresentMode::Mailbox,
    ];
    assert_eq!(VsyncMode::On.to_present_mode(&caps), wgpu::PresentMode::Fifo);
}

#[test]
fn vsync_off_prefers_immediate() {
    let caps = vec![
        wgpu::PresentMode::Fifo,
        wgpu::PresentMode::Immediate,
        wgpu::PresentMode::Mailbox,
    ];
    assert_eq!(VsyncMode::Off.to_present_mode(&caps), wgpu::PresentMode::Immediate);
}

#[test]
fn vsync_off_falls_back_to_mailbox() {
    let caps = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox];
    assert_eq!(VsyncMode::Off.to_present_mode(&caps), wgpu::PresentMode::Mailbox);
}

#[test]
fn vsync_off_falls_back_to_fifo() {
    let caps = vec![wgpu::PresentMode::Fifo];
    assert_eq!(VsyncMode::Off.to_present_mode(&caps), wgpu::PresentMode::Fifo);
}

#[test]
fn vsync_adaptive_prefers_auto_vsync() {
    let caps = vec![
        wgpu::PresentMode::Fifo,
        wgpu::PresentMode::AutoVsync,
    ];
    assert_eq!(VsyncMode::Adaptive.to_present_mode(&caps), wgpu::PresentMode::AutoVsync);
}

#[test]
fn vsync_adaptive_falls_back_to_fifo() {
    let caps = vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];
    assert_eq!(VsyncMode::Adaptive.to_present_mode(&caps), wgpu::PresentMode::Fifo);
}

#[test]
fn vsync_default_is_on() {
    assert_eq!(VsyncMode::default(), VsyncMode::On);
}

