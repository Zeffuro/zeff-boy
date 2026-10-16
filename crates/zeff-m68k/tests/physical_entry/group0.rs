use super::*;

pub(super) fn schedule(
    sp: u32,
    vector: u8,
    handler: u32,
    frame: [u16; 7],
    elapsed: &[u32],
    origin: u64,
) -> Vec<Event> {
    let mut clock = origin + 4;
    let mut events = vec![Event::Idle(origin, 4)];
    let offsets = [2, 6, 4, 8, 10, 14, 12];
    for (index, (offset, value)) in offsets.into_iter().zip(frame).enumerate() {
        events.push(Event::Write(
            access(
                sp.wrapping_sub(offset),
                FunctionCode::SupervisorData,
                AccessKind::Stack,
            ),
            value,
            clock,
        ));
        clock += u64::from(elapsed[index]);
    }
    for (index, (address, fc, kind)) in [
        (
            u32::from(vector) * 4,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        ),
        (
            u32::from(vector) * 4 + 2,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        ),
        (
            handler,
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        ),
        (
            handler.wrapping_add(2),
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 3 {
            events.push(Event::Idle(clock, 2));
            clock += 2;
        }
        events.push(Event::Read(access(address, fc, kind), clock));
        clock += u64::from(elapsed[index + 7]);
    }
    events
}

pub(super) fn frame(internal_pc: u32, sr: u16, ird: u16, address: u32, ssw: u16) -> [u16; 7] {
    [
        internal_pc as u16,
        sr,
        (internal_pc >> 16) as u16,
        ird,
        address as u16,
        ssw,
        (address >> 16) as u16,
    ]
}

fn assert_frame_bytes(device: &Device, sp: u32, frame: [u16; 7]) {
    let values = [
        frame[5], frame[6], frame[4], frame[3], frame[1], frame[2], frame[0],
    ];
    let actual: Vec<_> = (0..7)
        .flat_map(|index| {
            device.words[&(sp.wrapping_sub(14).wrapping_add(index * 2) & BUS_ADDRESS_MASK)]
                .to_be_bytes()
        })
        .collect();
    let expected: Vec<_> = values.into_iter().flat_map(u16::to_be_bytes).collect();
    assert_eq!(actual, expected);
}

#[test]
fn imported_capture_drives_every_frame_byte_fc_and_selected_ssw_bit() {
    let handler = 0x7654_3000;
    for vector in [2, 3] {
        for fc in [
            FunctionCode::UserData,
            FunctionCode::UserProgram,
            FunctionCode::SupervisorData,
            FunctionCode::SupervisorProgram,
        ] {
            for direction in [Direction::Read, Direction::Write] {
                for class in [
                    ProcessingClass::InstructionOrGroup2,
                    ProcessingClass::Group0OrGroup1,
                ] {
                    for sr in [TRACE | 0x0415, TRACE | SUPERVISOR | 0x061b] {
                        for ird in [0x001f, 0xffe0, 0xa55a] {
                            let mut captured = capture();
                            captured.vector = vector;
                            captured.function_code = fc;
                            captured.direction = direction;
                            captured.processing_class = class;
                            captured.phase_sr = sr;
                            captured.ird = ird;
                            let ssw = (ird & 0xffe0)
                                | fc as u16
                                | if direction == Direction::Read {
                                    0x10
                                } else {
                                    0
                                }
                                | if class == ProcessingClass::Group0OrGroup1 {
                                    8
                                } else {
                                    0
                                };
                            let initial = registers();
                            let mut cpu = imported(initial, captured, 113);
                            let mut bus = Device::new(113);
                            bus.handler(vector, handler);
                            let result = cpu.enter_imported_group0(&mut bus).unwrap();
                            let expected =
                                frame(captured.internal_pc, sr, ird, captured.logical_address, ssw);
                            assert_eq!(
                                bus.events,
                                schedule(initial.ssp, vector, handler, expected, &[4; 11], 113)
                            );
                            assert_frame_bytes(&bus, initial.ssp, expected);
                            assert_eq!(result.clocks, 50);
                            assert_eq!(result.kind, EntryKind::ImportedGroup0);
                            assert_eq!(cpu.clock(), CpuClocks(163));
                            assert_eq!(bus.clock, 163);
                            let mut final_registers = initial;
                            final_registers.ssp = initial.ssp - 14;
                            final_registers.pc = handler;
                            final_registers.set_status_register((sr | SUPERVISOR) & !TRACE);
                            assert_eq!(cpu.registers, final_registers);
                            assert_eq!(cpu.queued_words(), Some([0x70a5, 0x4e71]));
                            assert_eq!(
                                cpu.physical_inspection(),
                                PhysicalInspection::DiagnosticCompleteValues
                            );
                            assert_eq!(
                                cpu.physical_entry_state(),
                                PhysicalEntryState::Critical(EntryKind::ImportedGroup0)
                            );
                            assert_eq!(bus.ownership_edges, 0);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn total_elapsed_owns_each_wait_and_logical_addresses_wrap_independently() {
    let mut initial = registers();
    initial.ssp = 4;
    let captured = capture();
    let handler = 0xffff_fffe;
    let mut cpu = imported(initial, captured, 0x1234);
    let mut bus = Device::new(0x1234);
    bus.handler(captured.vector, handler);
    bus.elapsed = (0..11).map(|i| 4 + i * 3).collect();
    let result = cpu.enter_imported_group0(&mut bus).unwrap();
    let expected = frame(
        captured.internal_pc,
        captured.phase_sr,
        captured.ird,
        captured.logical_address,
        0xbad2,
    );
    assert_eq!(
        bus.events,
        schedule(4, 2, handler, expected, &bus.elapsed, 0x1234)
    );
    assert_frame_bytes(&bus, 4, expected);
    assert_eq!(cpu.registers.ssp, 0xffff_fff6);
    assert_eq!(result.clocks, 215);
    assert_eq!(cpu.clock().0, 0x1234 + 215);
    assert_eq!(cpu.clock().0, bus.clock);
    let transfers = bus.transfers();
    assert_eq!(transfers[1].0.logical_address, 0xffff_fffe);
    assert_eq!(transfers[1].0.bus_address, 0x00ff_fffe);
    assert_eq!(transfers[10].0.logical_address, 0);
    assert_eq!(transfers[10].0.bus_address, 0);
}

#[test]
fn pending_import_is_guarded_one_shot_and_its_snapshot_cannot_change() {
    let initial = registers();
    let mut cpu = imported(initial, capture(), 31);
    let mut bus = Device::new(31);
    assert_eq!(
        cpu.prime(&mut bus),
        Err(TimingFault::ImportedBoundaryPending)
    );
    assert_eq!(
        cpu.step_instruction(&mut bus),
        Err(TimingFault::ImportedBoundaryPending)
    );
    assert_eq!(
        cpu.step(&mut bus),
        Err(TimingFault::ImportedBoundaryPending)
    );
    assert_eq!(
        cpu.step_boundary(&mut bus),
        Err(TimingFault::ImportedBoundaryPending)
    );
    assert_eq!(
        cpu.service_pending_trace(&mut bus),
        Err(TimingFault::ImportedBoundaryPending)
    );
    assert!(bus.events.is_empty());
    cpu.registers.ssp = 0x1122_3344;
    assert_eq!(
        cpu.enter_imported_group0(&mut bus),
        Err(EntryFault::Input(EntryInputError::ChangedImportedSnapshot))
    );
    assert!(bus.events.is_empty());
    cpu.registers = initial;
    bus.handler(2, 0x1000);
    cpu.enter_imported_group0(&mut bus).unwrap();
    let before = bus.events.clone();
    assert_eq!(
        cpu.enter_imported_group0(&mut bus),
        Err(EntryFault::NoImportedBoundary)
    );
    assert_eq!(bus.events, before);
    let step = cpu.step_instruction(&mut bus).unwrap();
    assert_eq!(step.opcode, 0x70a5);
    assert_eq!(cpu.registers.d[0], 0xffff_ffa5);
    assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
}
