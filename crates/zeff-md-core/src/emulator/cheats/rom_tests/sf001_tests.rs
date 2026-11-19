use super::*;
use crate::cartridge::sf001::{Mapping, Revision};
use crate::hardware::bus::scheduled::ScheduledBus;
use crate::hardware::vdp::VdpDmaSource;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

mod sound;
mod state;

const REVISIONS: [Revision; 2] = [Revision::HiddenSave, Revision::UpperSave];
const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn machine(revision: Revision, region: Region, concurrent_sound: bool) -> Emulator {
    with_program(revision, region, concurrent_sound, &[0x60fe, 0x4e71])
}

fn with_program(
    revision: Revision,
    region: Region,
    concurrent_sound: bool,
    program: &[u16],
) -> Emulator {
    let mut bytes = crate::cartridge::sf001::tests::rom(revision);
    write_words(&mut bytes, 0x200, &[0x4ef9, 0xff, 0x200, 0x4e71]);
    for base in [0x1000, 0x381000] {
        write_words(&mut bytes, base, &[0x7001, 0x0640, 3, 0x60f8, 0x4e71]);
    }
    bytes[0x3ffffe..].fill(0);
    let sum = bytes[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, p| {
            sum.wrapping_add(u16::from_be_bytes([p[0], p[1]]))
        });
    let checksum = u16::from_be_bytes(bytes[0x18e..0x190].try_into().unwrap());
    bytes[0x3ffffe..].copy_from_slice(&checksum.wrapping_sub(sum).to_be_bytes());
    let mut m = Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    assert_eq!(m.bus.cartridge.sf001.as_ref().unwrap().revision(), revision);
    write_words(&mut m.bus.ram, 0x200, program);
    m.step_instruction().unwrap();
    m
}

fn control(m: &mut Emulator, value: u8) {
    assert!(
        m.bus
            .cartridge
            .sf001
            .as_mut()
            .unwrap()
            .control(0xe00, value)
    );
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn read(
    m: &mut Emulator,
    address: u32,
    lanes: ByteLanes,
) -> Result<u16, crate::hardware::bus::BusError> {
    m.bus
        .read_cycle(access(address, lanes), m.bus.cpu_clock)
        .result
}

fn advance(m: &mut Emulator, ticks: u64) {
    ScheduledBus::new(&mut m.bus, &mut m.sound_cpu, &mut m.sound_schedule)
        .advance_machine(ticks)
        .unwrap();
}

fn restore(m: &mut Emulator, raw: &[u8], front: &[u8], frontend: bool) {
    if frontend {
        m.decode_frontend_state(front).unwrap();
    } else {
        m.decode_state(raw).unwrap();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_main_lanes_aliases_save_and_disconnection_keep_distinct_owners() {
    for revision in REVISIONS {
        for region in REGIONS {
            for concurrent in [false, true] {
                let mut m = machine(revision, region, concurrent);
                let addresses = [
                    0x1000, 0x3fffe, 0x40000, 0x381000, 0x3bfffe, 0x3c0000, 0x3d0000, 0x3ffffe,
                ];
                let patches: Vec<_> = addresses
                    .into_iter()
                    .flat_map(|at| [patch(at, 0xa5), patch(at + 1, 0x5a)])
                    .collect();
                m.install_rom_patches(&patches).unwrap();
                m.install_rom_patches(
                    &[patches, vec![patch(0x1000, 0xc7), patch(0x1001, 0x7c)]].concat(),
                )
                .unwrap();
                control(&mut m, 0x80);
                let save = if revision == Revision::HiddenSave {
                    0x3c0000
                } else {
                    0x400000
                };
                m.bus
                    .write_cycle(access(save, ByteLanes::Both), 0xc76b, m.bus.cpu_clock)
                    .result
                    .unwrap();
                m.bus
                    .write_cycle(
                        access(save + 0xfffe, ByteLanes::Both),
                        0xe98d,
                        m.bus.cpu_clock,
                    )
                    .result
                    .unwrap();
                for mode in [0, 0x80, 0x40] {
                    for locked in [false, true] {
                        m.bus.cartridge.sf001.as_mut().unwrap().power_on();
                        control(&mut m, mode | if locked { 0x20 } else { 0 });
                        if locked {
                            control(&mut m, !mode);
                        }
                        for address in [
                            0x1000, 0x3fffe, 0x40000, 0x381000, 0x3bfffe, 0x3c0000, 0x3d0000,
                            0x3ffffe, 0x400000, 0x40fffe,
                        ] {
                            let sram = revision == Revision::UpperSave && address >= 0x400000
                                || revision == Revision::HiddenSave
                                    && mode == 0x80
                                    && (0x3c0000..0x400000).contains(&address);
                            let expected = if sram {
                                Some(if address & 0xffff == 0xfffe {
                                    0xe98d
                                } else {
                                    0xc76b
                                })
                            } else if mode == 0x40 || address >= 0x400000 {
                                None
                            } else if mode == 0 && address == 0x1000 {
                                Some(0xc77c)
                            } else {
                                Some(0xa55a)
                            };
                            match expected {
                                Some(word) => {
                                    assert_eq!(read(&mut m, address, ByteLanes::Both), Ok(word));
                                    assert_eq!(
                                        read(&mut m, address, ByteLanes::Upper),
                                        Ok(word & 0xff00)
                                    );
                                    assert_eq!(
                                        read(&mut m, address + 1, ByteLanes::Lower),
                                        Ok(word & 0xff)
                                    );
                                    assert_eq!(m.peek(address), Some((word >> 8) as u8));
                                }
                                None => {
                                    assert!(read(&mut m, address, ByteLanes::Both).is_err());
                                    assert_eq!(m.peek(address), None);
                                }
                            }
                            let offset = if sram || expected.is_none() {
                                None
                            } else {
                                Some(
                                    (if mode == 0x80 && address < 0x40000 {
                                        address + 0x380000
                                    } else {
                                        address
                                    }) as usize,
                                )
                            };
                            assert_eq!(m.bus.cartridge.rom_source_offset(address), offset);
                        }
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_identity_atomic_install_cold_vectors_and_live_reset_mapping() {
    for revision in REVISIONS {
        let mut m = machine(revision, Region::NtscUs, true);
        let source = m.bus.cartridge.bytes.clone();
        let digest = m.bus.cartridge.rom_digest();
        let header = m.bus.cartridge.header.clone();
        let cells = vec![0x5a; 0x10000];
        m.load_save_ram(&cells).unwrap();
        m.install_rom_patches(&patches("001001:07+381001:09+3FFFFF:A5"))
            .unwrap();
        let before = m.clone();
        for invalid in [
            patches("400000:FF"),
            patches("000004:0040+000006:0000"),
            patches("000007:01"),
        ] {
            assert!(m.install_rom_patches(&invalid).is_err());
            assert_eq!(m, before);
        }
        control(&mut m, 0xa0);
        m.install_rom_patches(&patches("000006:1000+001001:07+381001:09"))
            .unwrap();
        m.reset_cpu().unwrap();
        assert_eq!(m.queued_words(), Some([0x7009, 0x0640]));
        m.step_instruction().unwrap();
        assert_eq!(m.registers().d[0], 9);
        m.reset();
        assert_eq!(m.queued_words(), Some([0x7007, 0x0640]));
        assert_eq!(
            m.bus.cartridge.sf001.as_ref().unwrap().mapping(),
            Mapping::Linear
        );
        assert!(!m.bus.cartridge.sf001.as_ref().unwrap().locked());
        assert_eq!(m.dump_save_ram(), Some(cells));
        assert!(Arc::ptr_eq(&source, &m.bus.cartridge.bytes));
        assert_eq!(m.bus.cartridge.rom_digest(), digest);
        assert_eq!(m.bus.cartridge.header, header);
        assert!(
            Emulator::new_with_timing(source.to_vec(), LoadConfig::default(), TimingModel::Precise)
                .is_err()
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_sram_prefetch_keeps_bus_latch_without_claiming_rom_provenance() {
    use crate::hardware::bus::Prefetch;
    for revision in REVISIONS {
        let mut m = machine(revision, Region::NtscUs, true);
        control(&mut m, 0x80);
        m.load_save_ram(&vec![0x5a; 0x10000]).unwrap();
        m.install_rom_patches(&patches("3C0000:A55A")).unwrap();
        let address = if revision == Revision::HiddenSave {
            0x3c0000
        } else {
            0x400000
        };
        let mut read = access(address, ByteLanes::Both);
        read.kind = AccessKind::Prefetch;
        assert_eq!(m.bus.read_cycle(read, m.bus.cpu_clock).result, Ok(0x5a5a));
        assert_eq!(m.bus.cartridge.rom_source_offset(address), None);
        assert_eq!(
            m.bus.prefetch,
            if revision == Revision::HiddenSave {
                Prefetch::Full(0x5a5a)
            } else {
                Prefetch::Empty
            }
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_audio_origin_does_not_guess_source_of_queued_instruction_after_switch() {
    use zeff_emu_common::audio_trace::{MdTraceSource, MdTraceWrite};
    for revision in REVISIONS {
        for region in REGIONS {
            for concurrent_sound in [false, true] {
                let mut bytes = crate::cartridge::sf001::tests::rom(revision);
                write_words(&mut bytes, 0x200, &[0x1281, 0x1080, 0x60fe, 0x4e71]);
                write_words(&mut bytes, 0x380202, &[0x4e71]);
                bytes[0x3ffffe..].fill(0);
                let sum = bytes[0x200..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .fold(0_u16, |sum, p| {
                        sum.wrapping_add(u16::from_be_bytes([p[0], p[1]]))
                    });
                let checksum = u16::from_be_bytes(bytes[0x18e..0x190].try_into().unwrap());
                bytes[0x3ffffe..].copy_from_slice(&checksum.wrapping_sub(sum).to_be_bytes());
                let mut m = Emulator::new(
                    bytes,
                    LoadConfig {
                        region,
                        concurrent_sound,
                    },
                )
                .unwrap();
                m.reset_and_begin_audio_trace(8).unwrap();
                m.cpu.registers.a[0] = 0xc00011;
                m.cpu.registers.a[1] = 0xe00;
                m.cpu.registers.d[0] = 0x9f;
                m.cpu.registers.d[1] = 0x80;
                m.step_instruction().unwrap();
                assert_eq!(m.queued_words().unwrap()[0], 0x1080);
                assert_eq!(m.peek(0x202), Some(0x4e));
                assert_eq!(m.bus.cartridge.rom_source_offset(0x202), Some(0x380202));
                m.step_instruction().unwrap();
                let capture = m.finish_audio_trace().unwrap();
                capture.trace.validate_complete().unwrap();
                assert_eq!(capture.trace.events.len(), 1);
                match capture.trace.events[0].write {
                    MdTraceWrite::Psg { value, origin } => {
                        assert_eq!(value, 0x9f);
                        assert_eq!(origin.boundary_pc, 0x202);
                        assert_eq!(origin.source, MdTraceSource::Unknown);
                    }
                    other => panic!("unexpected trace write: {other:?}"),
                }
                assert_eq!(
                    crate::hardware::audio::trace::sound_origin(0x8202, 0, &m.bus.cartridge).source,
                    MdTraceSource::Unknown
                );
            }
        }
    }
}
