use super::*;
use crate::hardware::bus::scheduled::CompactBus;
use zeff_m68k::timing::InterruptAperture;

fn cycle(machine: &mut Emulator, route: u8, request: BusAccess, value: Option<u16>) -> Write {
    let clock = machine.bus.cpu_clock;
    let call = |bus: &mut dyn ClockedBus<Error = BusError>| match value {
        Some(value) if route.is_multiple_of(2) => bus.write_cycle(request, value, clock),
        Some(value) => bus.write_cycle_observed(request, value, clock).cycle,
        None if route.is_multiple_of(2) => {
            let response = bus.read_cycle(request, clock);
            CycleResult {
                result: response.result.map(|_| ()),
                wait_clocks: response.wait_clocks,
            }
        }
        None => {
            let response = bus
                .read_cycle_observed(request, clock, InterruptAperture::FollowingPrefetch)
                .cycle;
            CycleResult {
                result: response.result.map(|_| ()),
                wait_clocks: response.wait_clocks,
            }
        }
    };
    if route < 2 {
        call(&mut machine.bus)
    } else if route < 4 {
        call(&mut ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ))
    } else {
        let mut bus = CompactBus(ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ));
        let response = match value {
            Some(value) if route.is_multiple_of(2) => bus.write_cycle(request, value, clock),
            Some(value) => bus.write_cycle_observed(request, value, clock).cycle,
            None if route.is_multiple_of(2) => {
                let response = bus.read_cycle(request, clock);
                CycleResult {
                    result: response.result.map(|_| ()),
                    wait_clocks: response.wait_clocks,
                }
            }
            None => {
                let response = bus
                    .read_cycle_observed(request, clock, InterruptAperture::FollowingPrefetch)
                    .cycle;
                CycleResult {
                    result: response.result.map(|_| ()),
                    wait_clocks: response.wait_clocks,
                }
            }
        };
        CycleResult {
            result: response.result.map_err(|error| *error),
            wait_clocks: response.wait_clocks,
        }
    }
}

pub(super) fn check() {
    let mut digest = 0xcbf29ce484222325_u64;
    let mut cases = 0;
    for route in 0..6 {
        for (lanes, value, expected) in [
            (ByteLanes::Both, 0x8f02, 2),
            (ByteLanes::Upper, 0x8f01, 0x8f),
            (ByteLanes::Lower, 0xae8f, 0x8f),
        ] {
            let mut machine = ready(route >= 2);
            let address = 0xc00006 | u32::from(lanes == ByteLanes::Lower);
            assert_eq!(
                cycle(&mut machine, route, access(address, lanes), Some(value)).result,
                Ok(())
            );
            let pending = machine.vdp().registers().pending().unwrap();
            assert_eq!(
                (pending.index, pending.value, pending.publish_at.get()),
                (15, expected, 100)
            );
            assert_eq!(machine.vdp().registers().auto_increment(), Some(0));
            machine.cpu = PrefetchCpu::from_prefetched(
                machine.cpu.registers,
                machine.cpu.queued_words().unwrap(),
                machine.bus.cpu_clock,
            )
            .unwrap();
            let bytes = machine.encode_state().unwrap();
            let mut restored = ready(route >= 2);
            restored.decode_state(&bytes).unwrap();
            assert_eq!(restored.encode_state().unwrap(), bytes);
            let mut cloned = machine.clone();
            for future in [&mut machine, &mut restored, &mut cloned] {
                future.bus.advance_machine(16).unwrap();
                assert_eq!(future.vdp().registers().auto_increment(), Some(expected));
                assert_eq!(
                    cycle(future, route, access(0xc00004, ByteLanes::Both), None).result,
                    Ok(())
                );
                assert!(future.vdp().control().pending_word().is_none());
            }
            assert_eq!(restored.vdp(), machine.vdp());
            assert_eq!(restored.cpu, machine.cpu);
            assert_eq!(restored.bus.ram, machine.bus.ram);
            assert_eq!(restored.bus.now, machine.bus.now);
            assert_eq!(restored.bus.cpu_clock, machine.bus.cpu_clock);
            assert_eq!(cloned, machine);
            for byte in format!("{:?}", machine.vdp()).bytes() {
                digest = (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
            cases += 1;
        }
        let mut machine = ready(route >= 2);
        for word in [0x4000, 0x8f02] {
            assert_eq!(
                cycle(
                    &mut machine,
                    route,
                    access(0xc00004, ByteLanes::Both),
                    Some(word)
                )
                .result,
                Ok(())
            );
        }
        assert!(machine.vdp().control().pending_word().is_none());
        assert_eq!(machine.vdp().command().address, 0x8000);
        assert_eq!(machine.vdp().command().code, 1);
        assert_eq!(machine.vdp().registers().auto_increment(), Some(0));
        cases += 1;
    }
    for concurrent in [false, true] {
        let mut machine = program(
            &[0x23fc, 0x8f02, 0x8a00, 0x00c0, 4, 0x4e71, 0x4e71],
            concurrent,
        );
        let mut observed = machine.clone();
        let outcome = machine.step_instruction().unwrap();
        assert_eq!(observed.step_traced_boundary().unwrap(), outcome);
        assert_eq!(observed, machine);
        assert_eq!(machine.vdp().registers().auto_increment(), Some(2));
        assert_eq!(machine.vdp().registers().registers()[10], Some(0));
        assert!(machine.vdp().control().pending_word().is_none());
        cases += 1;
    }
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("main-control-payload-v1 {digest:016x} cases={cases}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("main-control-payload-v1 {digest:016x} cases={cases}");
}
