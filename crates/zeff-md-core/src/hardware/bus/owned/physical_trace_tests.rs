use super::*;
mod oracle;
use crate::sound_trace::physical::{Event, Fact, Journal, Shape};
use zeff_emu_common::save_state::StateWriter;
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::PrefetchCpu;

fn byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}
fn fixture(region: Region, sound: &[u8], bank: bool) -> (Bus, PrefetchCpu, usize) {
    let (mut bus, mut cpu, count) = fixture_program(region, sound, bank, true);
    for _ in 0..count - 2 {
        cpu.step_boundary(&mut bus.owned_context().unwrap())
            .unwrap();
    }
    let owner = bus.owned.as_ref().unwrap();
    let held = &owner.sound;
    assert_eq!(held.observed_controls(), (true, true));
    let pins = held.pins();
    assert!(
        !pins.bus_ack
            && !pins.mreq
            && !pins.read
            && !pins.write
            && !pins.m1
            && !pins.iorq
            && !pins.refresh
    );
    assert!(pins.address.is_none() && pins.data.is_none());
    assert_eq!(&held.memory().ram[..sound.len()], sound);
    assert_eq!(held.memory().ram[0x1000], 0x5a);
    assert_eq!(held.cpu().cycles(), 4);
    assert!(!held.bus_state().pending());
    assert!(owner.sound_event.is_none());
    assert!(bus.owned_fault().is_none());
    (bus, cpu, 2)
}

fn fixture_program(
    region: Region,
    sound: &[u8],
    bank: bool,
    restart: bool,
) -> (Bus, PrefetchCpu, usize) {
    let mut program = Vec::new();
    byte(&mut program, 0xa11100, 1);
    byte(&mut program, 0xa11200, 1);
    if bank {
        for _ in 0..9 {
            byte(&mut program, 0xa06000, 1);
        }
    }
    for (offset, &value) in sound.iter().enumerate() {
        byte(&mut program, 0xa00000 + offset as u32, value);
    }
    byte(&mut program, 0xa01000, 0x5a);
    if restart {
        for _ in 0..4 {
            byte(&mut program, 0xa11200, 0);
        }
        byte(&mut program, 0xa11200, 1);
    }
    byte(&mut program, 0xa11100, 0);
    let instructions = program.len() / 4;
    program.extend([0x60fe, 0x4e71]);
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in program.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut bus = Bus::new_owned(Cartridge::new(rom).unwrap(), region).unwrap();
    let mut cpu = PrefetchCpu::default();
    cpu.registers = bus.cartridge.effective_initial();
    cpu.prime(&mut bus.owned_context().unwrap()).unwrap();
    (bus, cpu, instructions)
}
fn raw(bus: &Bus) -> Vec<u8> {
    let mut writer = StateWriter::new();
    bus.write_state(&mut writer).unwrap();
    writer.into_bytes()
}
fn upload(bus: &mut Bus, cpu: &mut PrefetchCpu, count: usize, journal: &mut Journal) {
    for _ in 0..count {
        cpu.step_boundary(&mut bus.owned_context_capturing(&mut *journal).unwrap())
            .unwrap();
    }
}
fn advance(bus: &mut Bus, journal: &mut Journal) {
    let start = bus.cpu_clock;
    bus.owned_context_capturing(journal).unwrap().idle(start, 1);
    assert!(bus.owned_fault().is_none());
}
struct Retirement {
    complete: bool,
    consumed: Option<(u16, u8)>,
    before: u64,
    after: u64,
}

fn retired(event: &Event) -> Option<Retirement> {
    match event.fact {
        Fact::Retired(e) => Some(Retirement {
            complete: e.complete,
            consumed: e.consumed.map(|b| (b.address, b.value)),
            before: e.cpu_before,
            after: e.cpu_after,
        }),
        Fact::ClassifiedRetired(e) => Some(Retirement {
            complete: e.complete,
            consumed: e.consumed,
            before: e.cpu_before,
            after: e.cpu_after,
        }),
        _ => None,
    }
}
fn groups(journal: &Journal) -> Vec<Vec<u8>> {
    let mut groups = Vec::new();
    let mut bytes = Vec::new();
    for event in journal.events() {
        if let Some(retirement) = retired(event) {
            if let Some((_, byte)) = retirement.consumed {
                bytes.push(byte);
            }
            if retirement.complete && !bytes.is_empty() {
                groups.push(std::mem::take(&mut bytes));
            }
        }
    }
    groups
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_literal_uses_real_main_upload_and_outer_commits() {
    let program = [0x3e, 0x5a, 0x32, 0x00, 0x10, 0x00, 0x76];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let (mut bus, mut cpu, count) = fixture_program(region, &program, false, true);
        let mut ordinary = bus.clone();
        let mut ordinary_cpu = cpu.clone();
        let mut journal = Journal::default();
        journal.set_enabled(true);
        let mut baseline = None;
        for index in 0..count {
            cpu.step_boundary(&mut bus.owned_context_capturing(&mut journal).unwrap())
                .unwrap();
            ordinary_cpu
                .step_boundary(&mut ordinary.owned_context().unwrap())
                .unwrap();
            assert!(bus == ordinary);
            assert!(cpu == ordinary_cpu);
            assert!(raw(&bus) == raw(&ordinary));
            let owner = bus.owned.as_ref().unwrap();
            let sound = &owner.sound;
            if index == count - 7 {
                assert_eq!(sound.observed_controls(), (true, false));
                assert!(sound.pins().bus_ack && sound.memory().granted);
                assert_eq!(&sound.memory().ram[..program.len()], &program);
                assert_eq!(sound.memory().ram[0x1000], 0x5a);
                assert_eq!((sound.cpu().regs().pc, sound.cpu().cycles()), (1, 4));
                let startup: Vec<_> = journal.events().iter().filter_map(retired).collect();
                assert_eq!(startup.len(), 1);
                assert!(startup[0].complete);
                assert_eq!(startup[0].consumed, Some((0, 0)));
                assert_eq!((startup[0].before, startup[0].after), (0, 4));
                assert!(
                    journal
                        .events()
                        .iter()
                        .any(|event| matches!(event.fact, Fact::Sampled(0)))
                );
            }
            if index == count - 3 {
                assert_eq!(sound.observed_controls(), (true, true));
                let pins = sound.pins();
                assert!(
                    !pins.bus_ack
                        && !pins.mreq
                        && !pins.read
                        && !pins.write
                        && !pins.m1
                        && !pins.iorq
                        && !pins.refresh
                );
                assert!(pins.address.is_none() && pins.data.is_none());
                assert!(!sound.bus_state().pending());
                assert_eq!(sound.cpu().cycles(), 4);
                assert_eq!(&sound.memory().ram[..program.len()], &program);
                assert_eq!(sound.memory().ram[0x1000], 0x5a);
                assert!(owner.sound_event.is_none());
                assert!(bus.owned_fault().is_none());
                baseline = Some(sound.cpu().cycles());
                journal = Journal::default();
                journal.set_enabled(true);
            }
        }
        let baseline = baseline.unwrap();
        for _ in 0..128 {
            if groups(&journal).len() >= 4 {
                break;
            }
            advance(&mut bus, &mut journal);
            let start = ordinary.cpu_clock;
            ordinary.owned_context().unwrap().idle(start, 1);
        }
        assert_eq!(
            &groups(&journal)[..4],
            &[
                vec![0x3e, 0x5a],
                vec![0x32, 0x00, 0x10],
                vec![0x00],
                vec![0x76]
            ]
        );
        let writes: Vec<_> = journal
            .events()
            .iter()
            .filter_map(|event| match event.fact {
                Fact::LocalWrite { address, value, .. } => Some((address, value)),
                _ => None,
            })
            .collect();
        assert_eq!(writes, [(0x1000, 0x5a)]);
        assert!(bus == ordinary);
        assert!(cpu == ordinary_cpu);
        assert!(raw(&bus) == raw(&ordinary));
        assert!(journal.events().iter().all(|event| event.frame.is_none()));
        let terminal: Vec<_> = journal
            .events()
            .iter()
            .filter_map(retired)
            .filter(|retirement| retirement.complete)
            .map(|retirement| retirement.after)
            .take(4)
            .collect();
        assert_eq!(
            terminal
                .iter()
                .map(|clock| clock - baseline)
                .collect::<Vec<_>>(),
            [7, 20, 24, 28]
        );
        let fetches: Vec<_> = journal
            .events()
            .iter()
            .filter_map(|event| match event.fact {
                Fact::Activated(binding) if binding.shape == Shape::Fetch => Some(binding.address),
                _ => None,
            })
            .take(4)
            .collect();
        assert_eq!(fetches, [0, 2, 5, 6]);
        let sound = &bus.owned.as_ref().unwrap().sound;
        assert_eq!(sound.cpu().regs().a, 0x5a);
        assert!(sound.cpu().is_halted());
        assert_eq!(sound.memory().ram[0x1000], 0x5a);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_zero_clock_inputs_are_ordered_by_occurrence() {
    let (mut bus, mut cpu, count) = fixture(
        Region::NtscUs,
        &[0x09, 0x06, 2, 0x10, 0, 0x18, 0, 0x76],
        false,
    );
    let mut journal = Journal::default();
    journal.set_enabled(true);
    upload(&mut bus, &mut cpu, count, &mut journal);
    for _ in 0..128 {
        if groups(&journal).len() >= 5 {
            break;
        }
        advance(&mut bus, &mut journal);
    }
    assert_eq!(
        &groups(&journal)[..5],
        &[
            vec![0x09],
            vec![0x06, 2],
            vec![0x10, 0],
            vec![0x18, 0],
            vec![0x76]
        ]
    );
    let zero: Vec<_> = journal
        .events()
        .iter()
        .filter_map(|e| {
            retired(e)
                .filter(|retirement| {
                    retirement.consumed.is_some() && retirement.before == retirement.after
                })
                .map(|retirement| (e.occurrence.unwrap(), retirement.consumed.unwrap()))
        })
        .collect();
    assert!(zero.iter().any(|(_, (_, byte))| *byte == 0x09));
    assert!(zero.iter().any(|(_, (_, byte))| *byte == 0x10));
    assert!(
        zero.iter()
            .any(|(_, (address, byte))| *address == 4 && *byte == 0)
    );
    assert!(zero.windows(2).all(|pair| pair[0].0 != pair[1].0));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_detached_half_drop_publishes_nothing() {
    let (mut bus, mut cpu, count) = fixture(Region::NtscUs, &[0x3e, 0x5a, 0x76], false);
    let mut journal = Journal::default();
    journal.set_enabled(true);
    upload(&mut bus, &mut cpu, count, &mut journal);
    let before_bus = bus.clone();
    let before_events = journal.events().clone();
    let before_gaps = journal.gaps();
    let half = bus.vdp.owned_half().unwrap() + 1;
    let owner = bus.owned.as_ref().unwrap();
    let boundary = boundary::Boundary {
        physical: journal.begin_half(half),
        previous: (),
        board: owner.board.clone(),
        sound: owner.sound.clone(),
        fifo_witness: None,
        work_witness: None,
        audio: &bus.audio,
        read: protocol::ReadContext {
            region: bus.region,
            input: &bus.input,
            prefetch: bus.prefetch,
        },
    };
    let cartridge = &bus.cartridge;
    let prepared = bus
        .vdp
        .prepare_owned_half(boundary, &mut bus.ram, |address| {
            cartridge
                .read_sampled(address)
                .ok_or_else(|| anyhow::anyhow!("unmapped source"))
        })
        .unwrap();
    drop(prepared);
    assert_eq!(bus, before_bus);
    assert_eq!(*journal.events(), before_events);
    assert_eq!(journal.gaps(), before_gaps);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_work_pulse_joins_real_driver_and_commit_chronology() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut sound = vec![0; 8];
        sound.extend([0x21, 0, 0x80, 0x3e, 0x22, 0x77, 0x77, 0x76]);
        let (mut bus, mut cpu, count) = fixture(region, &sound, true);
        bus.ram[0x8000] = 0x11;
        let mut journal = Journal::default();
        journal.set_enabled(true);
        upload(&mut bus, &mut cpu, count, &mut journal);
        let mut witness = work_witness::Witness {
            companion_log: Some(Vec::new()),
            ..Default::default()
        };
        for _ in 0..256 {
            observed_advance(&mut bus, &mut journal, &mut witness);
            if groups(&journal).iter().any(|bytes| bytes == &[0x76]) {
                break;
            }
        }
        let access = BusAccess {
            logical_address: 0xff8000,
            bus_address: 0xff8000,
            lanes: ByteLanes::Upper,
            kind: AccessKind::Data,
            function_code: FunctionCode::SupervisorData,
        };
        let start = bus.cpu_clock;
        let mut context = bus.owned_context_capturing(&mut journal).unwrap();
        context.work_witness = Some(witness);
        context.write_cycle(access, 0x2200, start).result.unwrap();
        witness = context.work_witness.take().unwrap();
        let intervals = oracle::intervals(&witness);
        assert_eq!(intervals.len(), 2);
        let expected = oracle::expected(&witness, &intervals);
        let identities = oracle::exact(&journal, &intervals, &expected);
        assert!(identities.iter().all(Option::is_some));
        assert_ne!(identities[0], identities[1]);
        assert_eq!(bus.ram[0x8000], 0x22);
    }
}

fn observed_advance(bus: &mut Bus, journal: &mut Journal, witness: &mut work_witness::Witness) {
    let start = bus.cpu_clock;
    let mut context = bus.owned_context_capturing(journal).unwrap();
    context.work_witness = Some(std::mem::take(witness));
    context.idle(start, 1);
    *witness = context.work_witness.take().unwrap();
    assert!(context.state.fault.is_none());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_physical_journal_later_equal_work_and_psg_writes_lose_exhausted_identity() {
    for work in [true, false] {
        let address: u16 = if work { 0x8000 } else { 0x7f11 };
        let [low, high] = address.to_le_bytes();
        let mut sound = vec![0; 8];
        sound.extend([0x21, low, high, 0x3e, 0x22, 0x77, 0x77, 0x76]);
        let (mut bus, mut cpu, count) = fixture(Region::NtscUs, &sound, work);
        let mut journal = Journal::default();
        journal.set_enabled(true);
        upload(&mut bus, &mut cpu, count, &mut journal);
        let mut ordinary = bus.clone();
        let mut witness = work_witness::Witness {
            companion_log: Some(Vec::new()),
            ..Default::default()
        };
        let mut armed = false;
        for _ in 0..256 {
            if !armed
                && bus.owned_sound_cpu().unwrap().last_opcode_pc() == 13
                && bus.owned_sound_cpu().unwrap().instruction_bytes() == [0x77]
                && !journal.events().iter().any(
                    |e| matches!(e.fact, Fact::Activated(binding) if binding.shape == Shape::Write),
                )
            {
                journal.last_serial_next();
                armed = true;
            }
            observed_advance(&mut bus, &mut journal, &mut witness);
            let start = ordinary.cpu_clock;
            ordinary.owned_context().unwrap().idle(start, 1);
            if groups(&journal).iter().any(|bytes| bytes == &[0x76]) {
                break;
            }
        }
        assert!(armed);
        assert_eq!(bus, ordinary);
        assert_eq!(raw(&bus), raw(&ordinary));
        let intervals = oracle::intervals(&witness);
        assert_eq!(intervals.len(), 2);
        let ids = if work {
            let expected = oracle::expected(&witness, &intervals);
            oracle::exact(&journal, &intervals, &expected)
        } else {
            let receipts: Vec<_> = journal
                .events()
                .iter()
                .filter_map(|event| match event.fact {
                    Fact::PsgAccepted(receipt) => Some((event, receipt)),
                    _ => None,
                })
                .collect();
            assert_eq!(receipts.len(), 2);
            receipts
                .into_iter()
                .zip(&intervals)
                .map(|(item, interval)| {
                    let (event, receipt) = item;
                    oracle::psg_source(&journal, interval, event, receipt)
                })
                .collect()
        };
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].unwrap().serial, u64::MAX);
        assert_eq!(ids[1], None);
        assert!(journal.gaps() > 1);
    }
}
