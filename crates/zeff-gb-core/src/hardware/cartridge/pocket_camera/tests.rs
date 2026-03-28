use super::*;

fn make_camera() -> PocketCamera {
    let rom = vec![0u8; 0x10_0000];
    let mut cam = PocketCamera::new(rom, CAMERA_RAM_SIZE);
    cam.write_rom(0x0000, 0x0A);
    cam
}

#[test]
fn default_rom_bank_is_1() {
    let cam = make_camera();
    assert_eq!(cam.rom_bank, 1);
}

#[test]
fn rom_bank_0_is_valid() {
    let mut cam = make_camera();
    cam.write_rom(0x2000, 0);
    assert_eq!(cam.rom_bank, 0);
}

#[test]
fn normal_ram_bank_read_write() {
    let mut cam = make_camera();
    cam.write_rom(0x4000, 0x00);
    cam.write_ram(0xA000, 0x42);
    assert_eq!(cam.read_ram(0xA000), 0x42);

    cam.write_rom(0x4000, 0x01);
    cam.write_ram(0xA000, 0xBB);
    assert_eq!(cam.read_ram(0xA000), 0xBB);

    cam.write_rom(0x4000, 0x00);
    assert_eq!(cam.read_ram(0xA000), 0x42);
}

#[test]
fn sensor_bank_reads_regs() {
    let mut cam = make_camera();
    cam.write_rom(0x4000, SENSOR_BANK as u8);
    assert_eq!(cam.read_ram(0xA000), 0x00);
}

#[test]
fn capture_trigger_sets_active_bit() {
    let mut cam = make_camera();
    cam.write_rom(0x4000, SENSOR_BANK as u8);
    cam.write_ram(0xA000, 0x01);
    assert_eq!(cam.read_ram(0xA000) & 0x01, 0x01);
}

#[test]
fn capture_completes_after_stepping() {
    let mut cam = make_camera();
    cam.write_rom(0x4000, SENSOR_BANK as u8);
    cam.write_ram(0xA002, 0x00);
    cam.write_ram(0xA003, 0x01);
    cam.write_ram(0xA000, 0x01);

    cam.step(10_000);
    assert_eq!(cam.read_ram(0xA000) & 0x01, 0x00);
}

#[test]
fn normal_ram_access_works_without_ram_enable() {
    let cam = PocketCamera::new(vec![0; 0x10_0000], CAMERA_RAM_SIZE);
    // New compatibility behavior: photo RAM is readable even without RAMG writes.
    assert_eq!(cam.read_ram(0xA000), 0x00);
}

#[test]
fn sensor_bank_access_works_without_ram_enable() {
    let mut cam = PocketCamera::new(vec![0; 0x10_0000], CAMERA_RAM_SIZE);
    cam.write_rom(0x4000, SENSOR_BANK as u8);
    cam.write_ram(0xA002, 0x00);
    cam.write_ram(0xA003, 0x01);
    cam.write_ram(0xA000, 0x01);
    assert_eq!(cam.read_ram(0xA000) & 0x01, 0x01);
}

#[test]
fn busy_flag_clears_after_capture_time() {
    let mut cam = PocketCamera::new(vec![0; 0x10_0000], CAMERA_RAM_SIZE);
    cam.write_rom(0x4000, SENSOR_BANK as u8);
    cam.write_ram(0xA002, 0x00);
    cam.write_ram(0xA003, 0x02);
    cam.write_ram(0xA000, 0x01);
    assert_eq!(cam.read_ram(0xA000) & 0x01, 0x01);
    cam.step(10_000);
    assert_eq!(cam.read_ram(0xA000) & 0x01, 0x00);
}

#[test]
fn save_state_roundtrip() {
    let mut cam = make_camera();
    cam.write_rom(0x2000, 5);
    cam.write_ram(0xA000, 0x42);

    let mut writer = StateWriter::new();
    cam.write_state(&mut writer);
    let data = writer.into_bytes();
    let mut reader = StateReader::new(&data);
    let mut restored = PocketCamera::read_state(&mut reader).unwrap();
    restored.restore_rom_bytes(cam.rom.clone());

    assert_eq!(restored.read_rom(0x4000), cam.read_rom(0x4000));
    assert_eq!(restored.read_ram(0xA000), 0x42);
}
