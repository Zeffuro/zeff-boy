use super::*;

#[test]
fn post_boot_io_defaults_include_affine_bg_identity_matrices() {
    let bus = Bus::new(cartridge(), 48_000);

    assert_eq!(bus.read16(0x0400_0020), 0x0100);
    assert_eq!(bus.read16(0x0400_0022), 0);
    assert_eq!(bus.read16(0x0400_0024), 0);
    assert_eq!(bus.read16(0x0400_0026), 0x0100);
    assert_eq!(bus.read16(0x0400_0030), 0x0100);
    assert_eq!(bus.read16(0x0400_0032), 0);
    assert_eq!(bus.read16(0x0400_0034), 0);
    assert_eq!(bus.read16(0x0400_0036), 0x0100);
    assert_eq!(bus.read16(0x0400_0088), 0x0200);
}

#[test]
fn register_ram_reset_clears_requested_regions_and_forces_blank() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write8(0x0200_0000, 0x12);
    bus.write8(0x0300_0000, 0x34);
    bus.write8(0x0300_7F00, 0x56);
    bus.write16(0x0500_0000, 0x7FFF);
    bus.write16(0x0600_0000, 0x7FFF);
    bus.write16(0x0700_0000, 0x7FFF);
    bus.write16(0x0400_0200, 0x0001);
    bus.write32(0x0400_00B0, 0x0200_0000);

    bus.register_ram_reset(0xFF);

    assert_eq!(bus.read8(0x0200_0000), 0);
    assert_eq!(bus.read8(0x0300_0000), 0);
    assert_eq!(
        bus.read8(0x0300_7F00),
        0x56,
        "IWRAM stack area must be preserved"
    );
    assert_eq!(bus.read16(0x0500_0000), 0);
    assert_eq!(bus.read16(0x0600_0000), 0);
    assert_eq!(bus.read16(0x0700_0000), 0);
    assert_eq!(bus.read16(0x0400_0000), 0x0080);
    assert_eq!(bus.read16(0x0400_0200), 0);
    assert_eq!(bus.dma.channel(0).source, 0);
}

#[test]
fn if_register_clears_bits_written_as_one() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0202, 0xFFFF);
    bus.request_interrupt(0x0009);

    assert_eq!(bus.read16(0x0400_0202), 0x0009);

    bus.write16(0x0400_0202, 0x0001);

    assert_eq!(bus.read16(0x0400_0202), 0x0008);
}

#[test]
fn acknowledging_if_sets_bios_irq_flags_for_intr_wait() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.request_interrupt(INT_VBLANK | INT_HBLANK);

    bus.write16(0x0400_0202, INT_VBLANK);

    assert_eq!(bus.read16(0x0300_7FF8) & INT_VBLANK, INT_VBLANK);
    assert_eq!(bus.read16(0x0400_0202) & INT_VBLANK, 0);
    assert_eq!(bus.read16(0x0400_0202) & INT_HBLANK, INT_HBLANK);
}

#[test]
fn vblank_sets_if_when_dispstat_vblank_irq_enabled() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0004, 1 << 3);

    bus.step_cycles(1232 * 160);

    assert_ne!(bus.read16(0x0400_0202) & 1, 0);
    assert_ne!(bus.read16(0x0400_0004) & 1, 0);
}

#[test]
fn hblank_sets_if_when_dispstat_hblank_irq_enabled() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0004, 1 << 4);

    bus.step_cycles(1005);
    assert_eq!(bus.read16(0x0400_0202) & (1 << 1), 0);

    bus.step_cycles(1);

    assert_ne!(bus.read16(0x0400_0202) & (1 << 1), 0);
    assert_ne!(bus.read16(0x0400_0004) & (1 << 1), 0);
}

#[test]
fn hblank_status_toggles_during_hidden_vblank_lines() {
    let mut bus = Bus::new(cartridge(), 48_000);

    bus.step_cycles(1232 * 160);
    assert_ne!(bus.read16(0x0400_0004) & 1, 0);
    assert_eq!(bus.read16(0x0400_0004) & (1 << 1), 0);

    bus.step_cycles(1006);

    assert_ne!(bus.read16(0x0400_0004) & (1 << 1), 0);
}

#[test]
fn vblank_entry_reports_completed_scanline_frame() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 3);

    bus.write16(0x0600_0000, 0x001F);
    bus.step_cycles(1232 * 160);

    assert!(bus.ppu.frame_ready);
    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn scanline_renderer_uses_current_bg_scroll_per_line() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 1 << 8);
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    poke_vram8(&mut bus, 0x0600_0020, 0x22);
    poke_vram8(&mut bus, 0x0600_0024, 0x22);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0600_0802, 1);

    bus.write16(0x0400_0010, 0);
    bus.step_cycles(1006);
    bus.write16(0x0400_0010, 8);
    bus.step_cycles(226 + 1006);

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    let line_1 = SCREEN_WIDTH * 4;
    assert_eq!(
        &bus.ppu.framebuffer()[line_1..line_1 + 4],
        &[0x00, 0xFF, 0x00, 0xFF]
    );
}

#[test]
fn enabled_timer_irq_reports_pending_interrupt() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0200, 1 << 3);
    bus.write16(0x0400_0208, 1);
    bus.write16(0x0400_0100, 0xFFFF);
    bus.write16(0x0400_0102, 0x00C0);

    bus.step_cycles(2);

    assert_ne!(bus.read16(0x0400_0202) & (1 << 3), 0);
    assert!(bus.interrupt_pending());
}

#[test]
fn irq_handler_installed_accepts_low_bits_in_handler_pointer() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write32(0x03FF_FFFC, 0x0300_0163);
    bus.write32(0x0300_0160, 0xE3A0_0001);

    assert!(bus.irq_handler_installed());
}
