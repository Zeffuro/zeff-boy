use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, bail, ensure};
use sha2::{Digest, Sha256};

const SMS_MODE4_PRIORITY_HASH: &str =
    "7ec2e9fd0655256d47e7db086f04de771cbd9902b3e4ff9e173b1c7f08546beb";
const GG_MODE4_PRIORITY_HASH: &str =
    "7715a454e9af7c0e55496ff2587a4ee8a316fd146dd461dcd15adad01d4d32f6";
const SG_TMS_GRAPHICS_HASH: &str =
    "3a04c8207be376c1a0b06e9f138a3c81a8cb3780779cde35fd6d68d78e171267";
const SMS_CODEMASTERS_MAPPER_HASH: &str =
    "d9ed7af0b1095c1c251c295cb7d45bc2ae3f96fbe2b69110dc9d6c8c146be242";
const PCE_VDC_FETCH_CONTENTION_HASH: &str =
    "7edae12b94a85d7cf740d7fd86ce2a770051edc693efdda8504ed76f407c055d";
const PCE_CD_SYSCARD_HASH: &str =
    "4f85f6151a41a5b0244caa7fbb43cac8c67ceb596bcd6d6763028918d09cc81d";
const PCE_CD_DATA_HASH: &str = "8d68a7ed5321eab8e8e28f6f2e80f2b2931f841a5007fa1fa8451f7daef2983a";
const PCE_CD_CUE_HASH: &str = "bb8a1c396d686ee02f52a4122f538a2107f35bca29e26625727093a14584cb4d";
const PCE_CD_PACKAGE_IDENTITY_HASH: &str =
    "aa210f18f6f5820a9a3c68d843ed9a817f39b9003b93f587ed7d98ffd4798bd9";
const PCE_CD_DISC_IDENTITY_HASH: &str =
    "c8c7426b3f91d7bfb5f5029ffe18d8e2604195daf8f54c8b2494c2981e8f68a2";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixtureKind {
    Sega8,
    PceCdAdpcmIrq,
    PceVdcFetchContention,
}

impl FixtureKind {
    pub(crate) fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "sega8" => Ok(Self::Sega8),
            "pce-cd-adpcm-irq" => Ok(Self::PceCdAdpcmIrq),
            "pce-vdc-fetch-contention" => Ok(Self::PceVdcFetchContention),
            _ => bail!(
                "unknown fixture '{value}'; expected sega8, pce-cd-adpcm-irq, or pce-vdc-fetch-contention"
            ),
        }
    }
}

pub(crate) fn build_fixture(kind: FixtureKind, out_dir: &Path) -> anyhow::Result<()> {
    let artifacts = fixture_bytes(kind);
    for (name, bytes, expected_hash) in &artifacts {
        let actual_hash = sha256_hex(bytes);
        ensure!(
            actual_hash == *expected_hash,
            "internal {name} fixture hash changed: expected {expected_hash}, got {actual_hash}"
        );
    }
    if kind == FixtureKind::PceCdAdpcmIrq {
        let package_identity = pce_cd_package_identity(&artifacts[1].1, &artifacts[2].1);
        ensure!(
            package_identity == PCE_CD_PACKAGE_IDENTITY_HASH,
            "internal PCE-CD package identity changed: expected {PCE_CD_PACKAGE_IDENTITY_HASH}, got {package_identity}"
        );
        let disc_identity = pce_cd_disc_identity(&artifacts[1].1);
        ensure!(
            disc_identity == PCE_CD_DISC_IDENTITY_HASH,
            "internal PCE-CD disc identity changed: expected {PCE_CD_DISC_IDENTITY_HASH}, got {disc_identity}"
        );
    }
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create fixture directory {}", out_dir.display()))?;
    for (name, bytes, expected_hash) in artifacts {
        let path = out_dir.join(name);
        fs::write(&path, &bytes)
            .with_context(|| format!("failed to write fixture {}", path.display()))?;
        println!(
            "wrote {} bytes={} sha256={expected_hash}",
            path.display(),
            bytes.len()
        );
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn fixture_bytes(kind: FixtureKind) -> Vec<(&'static str, Vec<u8>, &'static str)> {
    match kind {
        FixtureKind::Sega8 => vec![
            (
                "sms-mode4-priority.sms",
                padded_sega8_rom(sms_priority_program()),
                SMS_MODE4_PRIORITY_HASH,
            ),
            (
                "gg-mode4-priority.gg",
                padded_sega8_rom(gg_priority_program()),
                GG_MODE4_PRIORITY_HASH,
            ),
            (
                "sg-tms-graphics.sg",
                padded_sega8_rom(sg_tms_program()),
                SG_TMS_GRAPHICS_HASH,
            ),
            (
                "sms-codemasters-mapper.sms",
                codemasters_mapper_rom(),
                SMS_CODEMASTERS_MAPPER_HASH,
            ),
        ],
        FixtureKind::PceCdAdpcmIrq => pce_cd_adpcm_irq_artifacts(),
        FixtureKind::PceVdcFetchContention => vec![(
            "vdc-fetch-contention.pce",
            pce_vdc_fetch_contention_rom(),
            PCE_VDC_FETCH_CONTENTION_HASH,
        )],
    }
}

fn push(bytes: &mut Vec<u8>, values: &[u8]) {
    bytes.extend_from_slice(values);
}
fn ld_a(bytes: &mut Vec<u8>, value: u8) {
    push(bytes, &[0x3e, value]);
}
fn out_a(bytes: &mut Vec<u8>, port: u8, value: u8) {
    ld_a(bytes, value);
    push(bytes, &[0xd3, port]);
}
fn vdp_register_write(bytes: &mut Vec<u8>, register: u8, value: u8) {
    out_a(bytes, 0xbf, value);
    out_a(bytes, 0xbf, 0x80 | register);
}
fn vdp_address(bytes: &mut Vec<u8>, address: u16, cram: bool) {
    out_a(bytes, 0xbf, address as u8);
    out_a(
        bytes,
        0xbf,
        ((address >> 8) as u8 & 0x3f) | if cram { 0xc0 } else { 0x40 },
    );
}
fn vram_write(bytes: &mut Vec<u8>, address: u16, values: &[u8]) {
    vdp_address(bytes, address, false);
    for &value in values {
        out_a(bytes, 0xbe, value);
    }
}
fn cram_write(bytes: &mut Vec<u8>, address: u16, values: &[u8]) {
    vdp_address(bytes, address, true);
    for &value in values {
        out_a(bytes, 0xbe, value);
    }
}
fn program_prefix(bytes: &mut Vec<u8>) {
    push(bytes, &[0xf3, 0x31, 0xf0, 0xdf]);
}
fn halt_loop(bytes: &mut Vec<u8>) {
    push(bytes, &[0x76, 0x18, 0xfd]);
}
fn mode4_setup(bytes: &mut Vec<u8>) {
    for (register, value) in [(0, 0x04), (1, 0x40), (2, 0x0e), (5, 0x7e), (7, 0)] {
        vdp_register_write(bytes, register, value);
    }
}
fn filled_mode4_tile(color: u8) -> [u8; 32] {
    let planes = [
        if color & 1 != 0 { 0xff } else { 0 },
        if color & 2 != 0 { 0xff } else { 0 },
        if color & 4 != 0 { 0xff } else { 0 },
        if color & 8 != 0 { 0xff } else { 0 },
    ];
    let mut tile = [0; 32];
    for row in 0..8 {
        tile[row * 4..row * 4 + 4].copy_from_slice(&planes);
    }
    tile
}
fn mode4_name_entry(bytes: &mut Vec<u8>, x: u16, y: u16, entry: u16) {
    vram_write(bytes, 0x3800 + (y * 32 + x) * 2, &entry.to_le_bytes());
}
fn mode4_sprite(bytes: &mut Vec<u8>, x: u8, y: u8, tile: u8) {
    vram_write(bytes, 0x3f00, &[y.wrapping_sub(1), 0xd0]);
    vram_write(bytes, 0x3f80, &[x, tile]);
}

fn sms_priority_program() -> Vec<u8> {
    let mut bytes = Vec::new();
    program_prefix(&mut bytes);
    mode4_setup(&mut bytes);
    let mut cram = vec![0, 3];
    cram.resize(17, 0);
    cram.push(0x30);
    cram_write(&mut bytes, 0, &cram);
    vram_write(&mut bytes, 0x20, &filled_mode4_tile(1));
    vram_write(&mut bytes, 0x40, &filled_mode4_tile(1));
    mode4_name_entry(&mut bytes, 2, 2, 0x1001);
    mode4_sprite(&mut bytes, 16, 16, 2);
    halt_loop(&mut bytes);
    bytes
}
fn gg_priority_program() -> Vec<u8> {
    let mut bytes = Vec::new();
    program_prefix(&mut bytes);
    mode4_setup(&mut bytes);
    let mut cram = vec![0; 36];
    cram[2] = 0x0f;
    cram[35] = 0x0f;
    cram_write(&mut bytes, 0, &cram);
    vram_write(&mut bytes, 0x20, &filled_mode4_tile(1));
    vram_write(&mut bytes, 0x40, &filled_mode4_tile(1));
    mode4_name_entry(&mut bytes, 7, 4, 0x1001);
    mode4_sprite(&mut bytes, 56, 32, 2);
    halt_loop(&mut bytes);
    bytes
}
fn sg_tms_program() -> Vec<u8> {
    let mut bytes = Vec::new();
    program_prefix(&mut bytes);
    for (register, value) in [
        (1, 0x40),
        (2, 0x0e),
        (3, 0x20),
        (4, 0),
        (5, 0x7f),
        (6, 0),
        (7, 1),
    ] {
        vdp_register_write(&mut bytes, register, value);
    }
    vram_write(&mut bytes, 8, &[0xff; 8]);
    vram_write(&mut bytes, 0x0800, &[0x60]);
    vram_write(&mut bytes, 0x3800 + (2 * 32 + 2), &[1]);
    vram_write(&mut bytes, 0x3f80, &[0xd0]);
    halt_loop(&mut bytes);
    bytes
}
fn padded_sega8_rom(mut program: Vec<u8>) -> Vec<u8> {
    program.resize(program.len().max(0x4000), 0);
    program
}
fn codemasters_mapper_rom() -> Vec<u8> {
    const BANK_SIZE: usize = 0x4000;
    let mut rom = vec![0; BANK_SIZE * 4];
    let mut boot = Vec::new();
    program_prefix(&mut boot);
    ld_a(&mut boot, 3);
    push(&mut boot, &[0x32, 0, 0x80, 0xc3, 0, 0x80]);
    rom[..boot.len()].copy_from_slice(&boot);
    let program = sms_priority_program();
    rom[BANK_SIZE * 3..BANK_SIZE * 3 + program.len()].copy_from_slice(&program);
    let header = 0x7fe0;
    rom[header] = 4;
    rom[header + 1..header + 6].copy_from_slice(&[0x31, 0x08, 0x93, 0x10, 0x59]);
    rom[header + 6..header + 8].copy_from_slice(&0x1234_u16.to_le_bytes());
    rom[header + 8..header + 10].copy_from_slice(&0xedcc_u16.to_le_bytes());
    rom
}

#[derive(Clone, Copy)]
enum FixupKind {
    Absolute,
    Relative,
}
struct Fixup {
    kind: FixupKind,
    offset: usize,
    label: &'static str,
}
struct PceAssembler {
    program: Vec<u8>,
    labels: BTreeMap<&'static str, usize>,
    fixups: Vec<Fixup>,
}
impl PceAssembler {
    fn new() -> Self {
        Self {
            program: Vec::new(),
            labels: BTreeMap::new(),
            fixups: Vec::new(),
        }
    }
    fn bytes(&mut self, values: &[u8]) {
        self.program.extend_from_slice(values);
    }
    fn byte(&mut self, value: u8) {
        self.program.push(value);
    }
    fn label(&mut self, label: &'static str) {
        assert!(self.labels.insert(label, self.program.len()).is_none());
    }
    fn absolute_label(&mut self, opcode: u8, label: &'static str) {
        self.byte(opcode);
        self.absolute_operand_label(label);
    }
    fn absolute_operand_label(&mut self, label: &'static str) {
        self.fixups.push(Fixup {
            kind: FixupKind::Absolute,
            offset: self.program.len(),
            label,
        });
        self.bytes(&[0, 0]);
    }
    fn relative_label(&mut self, opcode: u8, label: &'static str) {
        self.byte(opcode);
        self.fixups.push(Fixup {
            kind: FixupKind::Relative,
            offset: self.program.len(),
            label,
        });
        self.byte(0);
    }
    fn lda_immediate(&mut self, value: u8) {
        self.bytes(&[0xa9, value]);
    }
    fn lda_absolute(&mut self, address: u16) {
        self.bytes(&[0xad, address as u8, (address >> 8) as u8]);
    }
    fn sta_absolute(&mut self, address: u16) {
        self.bytes(&[0x8d, address as u8, (address >> 8) as u8]);
    }
    fn stz_absolute(&mut self, address: u16) {
        self.bytes(&[0x9c, address as u8, (address >> 8) as u8]);
    }
    fn lda_direct(&mut self, address: u8) {
        self.bytes(&[0xa5, address]);
    }
    fn sta_direct(&mut self, address: u8) {
        self.bytes(&[0x85, address]);
    }
    fn stz_direct(&mut self, address: u8) {
        self.bytes(&[0x64, address]);
    }
    fn vdc_register(&mut self, register: u8, value: u16) {
        self.bytes(&[0x03, register, 0x13, value as u8, 0x23, (value >> 8) as u8]);
    }
    fn finish(mut self, base: u16) -> (Vec<u8>, BTreeMap<&'static str, usize>) {
        for fixup in self.fixups {
            let target = self.labels[fixup.label];
            match fixup.kind {
                FixupKind::Absolute => self.program[fixup.offset..fixup.offset + 2]
                    .copy_from_slice(&(base + target as u16).to_le_bytes()),
                FixupKind::Relative => {
                    let delta = target as isize - (fixup.offset + 1) as isize;
                    assert!((-128..=127).contains(&delta));
                    self.program[fixup.offset] = delta as i8 as u8;
                }
            }
        }
        (self.program, self.labels)
    }
}

fn pce_vdc_fetch_contention_rom() -> Vec<u8> {
    const BASE: u16 = 0xe000;
    const STATUS: u16 = 0x2000;
    const ROW: u16 = 0x2015;
    const FRAME: u16 = 0x2016;
    const SOURCE: u16 = 0x2100;
    const LOW: u16 = 2;
    const HIGH: u16 = 3;
    const VCE: [u16; 4] = [0x0402, 0x0403, 0x0404, 0x0405];
    let mut a = PceAssembler::new();
    a.label("reset");
    a.bytes(&[0x78, 0xd8, 0xd4, 0xa2, 0xff, 0x9a]);
    a.lda_immediate(0xff);
    a.bytes(&[0x53, 1]);
    a.lda_immediate(0xf8);
    a.bytes(&[0x53, 2]);
    for (index, value) in [
        b'Z', b'P', b'C', b'E', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, b'V', b'D', b'C', b'S', 1, 0, 0,
        0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ]
    .iter()
    .enumerate()
    {
        a.lda_immediate(*value);
        a.sta_absolute(STATUS + index as u16);
    }
    for index in 0..16 {
        a.lda_immediate((index * 0x11) as u8);
        a.sta_absolute(SOURCE + index);
    }
    a.stz_absolute(0x0400);
    for address in VCE {
        a.stz_absolute(address);
    }
    for (register, value) in [
        (0x05, 0),
        (0x07, 0),
        (0x08, 0),
        (0x09, 0),
        (0x0a, 0x0202),
        (0x0b, 0x041f),
        (0x0c, 0x1702),
        (0x0d, 0x00df),
        (0x0e, 0x000a),
        (0x06, 0),
        (0x05, 0x008c),
    ] {
        a.vdc_register(register, value);
    }
    a.stz_absolute(0x1402);
    a.stz_absolute(0x1403);
    a.byte(0x58);
    a.label("main_spin");
    a.relative_label(0x80, "main_spin");
    a.label("irq1");
    a.lda_absolute(0);
    a.bytes(&[0x29, 0x20]);
    a.relative_label(0xf0, "raster");
    a.absolute_label(0x4c, "vblank");
    a.label("raster");
    a.lda_absolute(ROW);
    a.bytes(&[0x4a; 4]);
    a.bytes(&[0x29, 3, 0xaa, 0xbd]);
    a.absolute_operand_label("mwr_modes");
    a.bytes(&[0x03, 0x09]);
    a.sta_absolute(LOW);
    a.stz_absolute(HIGH);
    a.bytes(&[0x03, 0]);
    a.stz_absolute(LOW);
    a.lda_immediate(0x30);
    a.sta_absolute(HIGH);
    a.bytes(&[0x03, 2]);
    for address in VCE {
        a.stz_absolute(address);
    }
    a.bytes(&[
        0xe3,
        SOURCE as u8,
        (SOURCE >> 8) as u8,
        LOW as u8,
        (LOW >> 8) as u8,
        0x10,
        0,
    ]);
    a.lda_absolute(ROW);
    a.bytes(&[0x4a; 4]);
    a.bytes(&[0x29, 3, 0xaa, 0xbd]);
    a.absolute_operand_label("color_low");
    a.stz_absolute(0x0402);
    a.stz_absolute(0x0403);
    a.sta_absolute(0x0404);
    a.byte(0xbd);
    a.absolute_operand_label("color_high");
    a.sta_absolute(0x0405);
    a.bytes(&[0xee, ROW as u8, (ROW >> 8) as u8]);
    a.lda_absolute(ROW);
    a.bytes(&[0xc9, 0x40]);
    a.relative_label(0x90, "schedule_next");
    a.lda_immediate(1);
    a.sta_absolute(STATUS + 4);
    a.lda_immediate(0x0f);
    a.sta_absolute(STATUS + 5);
    for address in [STATUS + 6, STATUS + 9, STATUS + 12] {
        a.lda_immediate(0x10);
        a.sta_absolute(address);
    }
    a.lda_immediate(0x0f);
    a.sta_absolute(STATUS + 20);
    a.bytes(&[0x03, 6]);
    a.stz_absolute(LOW);
    a.stz_absolute(HIGH);
    a.byte(0x40);
    a.label("schedule_next");
    a.bytes(&[0x0a, 0x18, 0x69, 0x60, 0x03, 6]);
    a.sta_absolute(LOW);
    a.stz_absolute(HIGH);
    a.byte(0x40);
    a.label("vblank");
    a.stz_absolute(ROW);
    a.bytes(&[0xee, FRAME as u8, (FRAME >> 8) as u8]);
    a.bytes(&[0x03, 6]);
    a.lda_immediate(0x60);
    a.sta_absolute(LOW);
    a.stz_absolute(HIGH);
    a.byte(0x40);
    a.label("unexpected");
    a.byte(0x40);
    a.label("mwr_modes");
    a.bytes(&[0, 1, 2, 3]);
    a.label("color_low");
    a.bytes(&[0x38, 7, 0xc0, 0xff]);
    a.label("color_high");
    a.bytes(&[0, 0, 1, 1]);
    let (program, labels) = a.finish(BASE);
    assert!(program.len() <= 0x1ff6);
    let mut rom = vec![0xea; 8 * 1024];
    rom[..program.len()].copy_from_slice(&program);
    for (offset, target) in [
        (0x1ff6, BASE + labels["unexpected"] as u16),
        (0x1ff8, BASE + labels["irq1"] as u16),
        (0x1ffa, BASE + labels["unexpected"] as u16),
        (0x1ffc, BASE + labels["unexpected"] as u16),
        (0x1ffe, BASE),
    ] {
        rom[offset..offset + 2].copy_from_slice(&target.to_le_bytes());
    }
    rom
}

fn cd_wait_request(assembler: &mut PceAssembler, label: &'static str) {
    assembler.label(label);
    assembler.lda_absolute(0xd800);
    assembler.bytes(&[0x29, 0x40]);
    assembler.relative_label(0xf0, label);
}

fn cd_acknowledge(assembler: &mut PceAssembler) {
    assembler.lda_immediate(0x80);
    assembler.sta_absolute(0xd802);
    assembler.stz_absolute(0xd802);
}

fn cd_fail_jump(assembler: &mut PceAssembler, code: u8, continue_label: &'static str) {
    assembler.relative_label(0xf0, continue_label);
    assembler.lda_immediate(code);
    assembler.absolute_label(0x4c, "fail");
    assembler.label(continue_label);
}

fn pce_cd_adpcm_irq_artifacts() -> Vec<(&'static str, Vec<u8>, &'static str)> {
    let (system_card, data, cue) = pce_cd_adpcm_irq_bytes();
    vec![
        ("syscard3.pce", system_card, PCE_CD_SYSCARD_HASH),
        ("cd-adpcm-irq.bin", data, PCE_CD_DATA_HASH),
        ("cd-adpcm-irq.cue", cue, PCE_CD_CUE_HASH),
    ]
}

fn pce_cd_adpcm_irq_bytes() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    const BASE: u16 = 0xe000;
    const CD: u16 = 0xd800;
    const SECTOR_COUNT: u8 = 17;
    let mut a = PceAssembler::new();

    a.label("reset");
    a.bytes(&[0x78, 0xd8, 0xd4]);
    a.lda_immediate(0xf8);
    a.bytes(&[0x53, 3]);
    a.lda_immediate(0xff);
    a.bytes(&[0x53, 0x40, 0xa2, 0xff, 0x9a]);
    for (address, value) in [(0, b'Z'), (1, b'P'), (2, b'C'), (3, b'E')] {
        a.lda_immediate(value);
        a.sta_direct(address);
    }
    for address in 4..=14 {
        a.stz_direct(address);
    }
    a.lda_immediate(0x80);
    a.sta_absolute(CD + 13);
    a.stz_absolute(CD + 8);
    a.stz_absolute(CD + 9);
    a.lda_immediate(3);
    a.sta_absolute(CD + 13);
    a.stz_absolute(CD);
    for (index, command) in [0x08, 0, 0, 0, SECTOR_COUNT, 0].iter().enumerate() {
        cd_wait_request(
            &mut a,
            match index {
                0 => "command_request_0",
                1 => "command_request_1",
                2 => "command_request_2",
                3 => "command_request_3",
                4 => "command_request_4",
                5 => "command_request_5",
                _ => unreachable!(),
            },
        );
        a.lda_immediate(*command);
        a.sta_absolute(CD + 1);
        cd_acknowledge(&mut a);
    }
    a.lda_immediate(2);
    a.sta_absolute(CD + 11);

    a.label("wait_status");
    a.lda_absolute(CD);
    a.bytes(&[0x29, 0xd8, 0xc9, 0xd8]);
    a.relative_label(0xd0, "wait_status");
    a.lda_absolute(CD + 1);
    a.bytes(&[0xc9, 0]);
    cd_fail_jump(&mut a, 0x81, "status_ok");
    cd_acknowledge(&mut a);
    cd_wait_request(&mut a, "message_request");
    a.lda_absolute(CD + 1);
    a.bytes(&[0xc9, 0]);
    cd_fail_jump(&mut a, 0x82, "message_ok");
    cd_acknowledge(&mut a);

    a.label("wait_bus_free");
    a.lda_absolute(CD);
    a.relative_label(0x30, "wait_bus_free");
    a.stz_absolute(CD + 13);
    a.stz_absolute(CD + 8);
    a.stz_absolute(CD + 9);
    a.lda_immediate(0x0c);
    a.sta_absolute(CD + 13);
    a.lda_absolute(CD + 10);
    a.bytes(&[0xc9, 0x5a]);
    cd_fail_jump(&mut a, 0x83, "first_byte_ok");

    a.stz_absolute(CD + 13);
    a.lda_immediate(0xff);
    a.sta_absolute(CD + 8);
    a.lda_immediate(0x87);
    a.sta_absolute(CD + 9);
    a.lda_immediate(0x0c);
    a.sta_absolute(CD + 13);
    a.lda_absolute(CD + 10);
    a.bytes(&[0xc9, 0x49]);
    cd_fail_jump(&mut a, 0x84, "last_byte_ok");

    a.stz_absolute(CD + 13);
    a.stz_absolute(CD + 8);
    a.stz_absolute(CD + 9);
    a.lda_immediate(0x0c);
    a.sta_absolute(CD + 13);
    a.lda_immediate(0x0f);
    a.sta_absolute(CD + 14);
    a.lda_immediate(0x0c);
    a.sta_absolute(CD + 2);
    a.lda_immediate(0x60);
    a.sta_absolute(CD + 13);
    a.lda_immediate(0x0e);
    a.sta_absolute(CD + 15);
    a.lda_absolute(CD + 15);
    a.bytes(&[0xc9, 0x0e]);
    cd_fail_jump(&mut a, 0x87, "fade_start_ok");
    a.byte(0x58);

    a.label("count_loop");
    a.bytes(&[0xe6, 6]);
    a.relative_label(0xd0, "count_done");
    a.bytes(&[0xe6, 7]);
    a.relative_label(0xd0, "count_done");
    a.bytes(&[0xe6, 8]);
    a.label("count_done");
    a.lda_direct(5);
    a.bytes(&[0xc9, 3]);
    a.relative_label(0xd0, "count_loop");
    a.byte(0x78);

    a.lda_direct(11);
    a.relative_label(0xf0, "half_high_ok");
    a.lda_immediate(0x85);
    a.absolute_label(0x4c, "fail");
    a.label("half_high_ok");
    a.lda_direct(10);
    a.bytes(&[0xc9, 0x20]);
    a.relative_label(0xb0, "half_timing_ok");
    a.lda_immediate(0x85);
    a.absolute_label(0x4c, "fail");
    a.label("half_timing_ok");
    a.lda_direct(14);
    a.bytes(&[0xc9, 8]);
    a.relative_label(0xb0, "end_min_ok");
    a.lda_immediate(0x85);
    a.absolute_label(0x4c, "fail");
    a.label("end_min_ok");
    a.lda_direct(14);
    a.bytes(&[0xc9, 0x20]);
    a.relative_label(0x90, "timing_ok");
    a.lda_immediate(0x85);
    a.absolute_label(0x4c, "fail");
    a.label("timing_ok");
    a.lda_absolute(CD + 15);
    a.bytes(&[0xc9, 0x0e]);
    cd_fail_jump(&mut a, 0x88, "fade_latch_ok");
    a.lda_immediate(1);
    a.sta_direct(4);
    a.label("pass_spin");
    a.relative_label(0x80, "pass_spin");

    a.label("fail");
    a.byte(0x78);
    a.stz_absolute(CD + 2);
    a.sta_direct(4);
    a.label("fail_spin");
    a.relative_label(0x80, "fail_spin");

    a.label("irq2");
    a.byte(0x48);
    a.lda_absolute(CD + 3);
    a.bytes(&[0x29, 4]);
    a.relative_label(0xf0, "check_end_irq");
    a.lda_direct(5);
    a.relative_label(0xd0, "unexpected_irq");
    a.lda_immediate(1);
    a.sta_direct(5);
    for index in 0..3 {
        a.lda_direct(6 + index);
        a.sta_direct(9 + index);
    }
    a.lda_immediate(8);
    a.sta_absolute(CD + 2);
    a.bytes(&[0x68, 0x40]);
    a.label("check_end_irq");
    a.lda_absolute(CD + 3);
    a.bytes(&[0x29, 8]);
    a.relative_label(0xf0, "unexpected_irq");
    a.lda_direct(5);
    a.bytes(&[0xc9, 1]);
    a.relative_label(0xd0, "unexpected_irq");
    a.lda_immediate(3);
    a.sta_direct(5);
    for index in 0..3 {
        a.lda_direct(6 + index);
        a.sta_direct(12 + index);
    }
    a.stz_absolute(CD + 2);
    a.bytes(&[0x68, 0x40]);
    a.label("unexpected_irq");
    a.stz_absolute(CD + 2);
    a.byte(0x78);
    a.lda_immediate(0x86);
    a.sta_direct(4);
    a.label("irq_fail_spin");
    a.relative_label(0x80, "irq_fail_spin");

    let (program, labels) = a.finish(BASE);
    let mut system_card = vec![0xea; 256 * 1024];
    system_card[..program.len()].copy_from_slice(&program);
    for (offset, target) in [
        (0x1ff6, BASE + labels["irq2"] as u16),
        (0x1ff8, BASE + labels["unexpected_irq"] as u16),
        (0x1ffa, BASE + labels["unexpected_irq"] as u16),
        (0x1ffc, BASE + labels["unexpected_irq"] as u16),
        (0x1ffe, BASE),
    ] {
        system_card[offset..offset + 2].copy_from_slice(&target.to_le_bytes());
    }
    let data = (0..2048 * 17)
        .map(|index| (index * 17 + 0x5a) as u8)
        .collect();
    let cue =
        b"FILE \"cd-adpcm-irq.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00".to_vec();
    (system_card, data, cue)
}

fn pce_cd_package_identity(data: &[u8], cue: &[u8]) -> String {
    let mut identity = Vec::new();
    for bytes in [b"zeff-boy:pce-cd-data:v2".as_slice(), cue] {
        identity.extend_from_slice(&(bytes.len() as i64).to_le_bytes());
        identity.extend_from_slice(bytes);
    }
    identity.extend_from_slice(&1_i64.to_le_bytes());
    for bytes in [b"cd-adpcm-irq.bin".as_slice(), data] {
        identity.extend_from_slice(&(bytes.len() as i64).to_le_bytes());
        identity.extend_from_slice(bytes);
    }
    sha256_hex(&identity)
}

fn pce_cd_disc_identity(data: &[u8]) -> String {
    let mut identity = Vec::new();
    identity.extend_from_slice(b"zeff-boy:pce-core-cd-disc:v1");
    identity.push(0);
    identity.extend_from_slice(&1_u32.to_le_bytes());
    identity.extend_from_slice(&[1, 4, 0]);
    identity.extend_from_slice(&0_u32.to_le_bytes());
    identity.extend_from_slice(&0_u32.to_le_bytes());
    identity.push(1);
    identity.extend_from_slice(&(data.len() as i64).to_le_bytes());
    identity.extend_from_slice(data);
    sha256_hex(&identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pce_cd_fixture_identities_are_stable() {
        let (_, data, cue) = pce_cd_adpcm_irq_bytes();
        assert_eq!(
            pce_cd_package_identity(&data, &cue),
            PCE_CD_PACKAGE_IDENTITY_HASH
        );
        assert_eq!(pce_cd_disc_identity(&data), PCE_CD_DISC_IDENTITY_HASH);
    }
}
