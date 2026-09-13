use super::*;

fn source_event(machine: &mut PceMachine, pc: u16) -> AudioTraceSource {
    machine.cpu.cpu_mut().registers_mut().pc = pc;
    run(machine, 2);
    let trace = finish(machine);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].pc, u32::from(pc + 2));
    trace.events[0].instruction_source
}

#[test]
fn plain_hucard_mirrors_resolve_to_the_bytes_that_were_fetched() {
    for (image_len, page, offset) in [
        (0x60000, 0x20, 0),
        (0x60000, 0x40, 0x40000),
        (0x60000, 0x7F, 0x5E000),
        (0x80000, 0x40, 0x40000),
        (0x80000, 0x7F, 0x7E000),
    ] {
        let mut image = vec![0xEA; image_len];
        image[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
        image[offset..offset + 5].copy_from_slice(&[0xA9, 255, 0x8D, 1, 8]);
        let mut machine = PceMachine::new(image).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
        machine.cpu_mut().cpu_mut().set_mapping_register(2, page);
        machine.reset_and_begin_audio_trace(1).unwrap();
        assert_eq!(
            source_event(&mut machine, 0x4000),
            AudioTraceSource::CartridgeRom {
                offset: (offset + 2) as u64,
                bit_reversed: false
            }
        );
    }
}

#[test]
fn sf2_banked_code_uses_the_selected_original_rom_offset() {
    let mut image = vec![0xEA; 0x280000];
    image[..13].copy_from_slice(&[
        0xA9, 0, 0x53, 8, 0x8D, 0xF3, 0x7F, 0xA9, 0x40, 0x53, 4, 0x4C, 0,
    ]);
    image[13] = 0x40;
    image[0x200000..0x200005].copy_from_slice(&[0xA9, 255, 0x8D, 1, 8]);
    image[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Sf2Ce);
    let mut machine = PceMachine::with_cartridge(image, descriptor).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine.reset_and_begin_audio_trace(1).unwrap();
    run(&mut machine, 8);
    let trace = finish(&mut machine);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].pc, 0x4002);
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::CartridgeRom {
            offset: 0x200002,
            bit_reversed: false
        }
    );
}

#[test]
fn ram_writers_are_identified_without_inventing_rom_provenance() {
    for topology in [PceHardwareTopology::Base, PceHardwareTopology::SuperGrafx] {
        let descriptor = PceCartridgeDescriptor::default().with_required_hardware(match topology {
            PceHardwareTopology::Base => PceCartridgeHardware::Base,
            PceHardwareTopology::SuperGrafx => PceCartridgeHardware::SuperGrafx,
        });
        let mut machine = PceMachine::with_cartridge(rom(&[]), descriptor).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
        machine.cpu_mut().cpu_mut().set_mapping_register(2, 0xFB);
        let offset = if topology == PceHardwareTopology::Base {
            0
        } else {
            0x6000
        };
        machine.mapped_work_ram_mut()[offset..offset + 5].copy_from_slice(&[0xA9, 255, 0x8D, 1, 8]);
        machine.reset_and_begin_audio_trace(1).unwrap();
        assert_eq!(
            source_event(&mut machine, 0x4000),
            AudioTraceSource::WorkRam {
                offset: offset as u32 + 2
            }
        );
    }

    let mut image = vec![0xEA; 0x80000];
    image[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Populous);
    let mut machine = PceMachine::with_cartridge(image, descriptor).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0x42);
    for (offset, byte) in [0xA9, 255, 0x8D, 1, 8].into_iter().enumerate() {
        machine.bus.write(0x84000 + offset as u32, byte);
    }
    machine.reset_and_begin_audio_trace(1).unwrap();
    assert_eq!(
        source_event(&mut machine, 0x4000),
        AudioTraceSource::CartridgeRam { offset: 0x4002 }
    );
}

#[test]
fn unmapped_and_mmio_code_do_not_claim_cartridge_offsets() {
    let mut machine = machine(&[]);
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0x10);
    assert_eq!(
        machine.audio_trace_source(0x4000),
        AudioTraceSource::Unmapped
    );
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0xFF);
    assert_eq!(
        machine.audio_trace_source(0x4800),
        AudioTraceSource::Unknown
    );
    machine.cpu_mut().cpu_mut().set_mapping_register(2, 0x90);
    assert_eq!(
        machine.audio_trace_source(0x4000),
        AudioTraceSource::Unmapped
    );
}

#[test]
fn block_writer_source_is_fixed_before_its_own_mapper_writes() {
    let mut image = vec![0; 0x280000];
    image[..11].copy_from_slice(&[0xA9, 0x40, 0x53, 4, 0xA9, 0xFF, 0x53, 0x10, 0x4C, 0, 0x40]);
    image[0x80000..0x80007].copy_from_slice(&[0x73, 0, 0xC0, 0xF0, 0x7F, 0x11, 8]);
    image[0x1FFE..0x2000].copy_from_slice(&0xE000_u16.to_le_bytes());
    let descriptor = PceCartridgeDescriptor::default().with_hucard_board(PceHuCardBoard::Sf2Ce);
    let mut machine = PceMachine::with_cartridge(image, descriptor).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0xF8);
    machine.reset_and_begin_audio_trace(1).unwrap();
    run(&mut machine, 6);
    assert_eq!(machine.bus.hucard_mapping_token(), 3);
    let trace = finish(&mut machine);
    assert_eq!(trace.events.len(), 1);
    assert_eq!(trace.events[0].pc, 0x4000);
    assert_eq!(trace.events[0].write.register, 0);
    assert_eq!(
        trace.events[0].instruction_source,
        AudioTraceSource::CartridgeRom {
            offset: 0x80000,
            bit_reversed: false
        }
    );
    assert_eq!(machine.bus.hucard_rom_offset(0x80000), Some(0x200000));
}
