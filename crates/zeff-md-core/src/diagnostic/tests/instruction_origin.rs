use super::*;
use zeff_m68k::timing::InterruptAperture;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_fetch_keeps_sampled_sources_irq_and_exactly_one_read_record() {
    let terminal = machine();
    for port_history in [false, true] {
        for observed in [false, true] {
            for failure in [false, true] {
                let mut direct = Bus::new(
                    Cartridge::new(terminal.rom_bytes().to_vec()).unwrap(),
                    crate::Region::NtscUs,
                );
                direct.irq_level = 4;
                if !port_history {
                    let registers = *direct.vdp.registers();
                    let interrupts = crate::hardware::vdp::VdpInterrupts::at_counters_zero(
                        direct.now, 0, false, false,
                    );
                    direct.vdp =
                        crate::Vdp::from_known_state(interrupts, registers, direct.region).unwrap();
                }
                assert_eq!(direct.vdp.interrupts().has_port_history(), port_history);
                let mut traced = direct.clone();
                let access = BusAccess {
                    logical_address: 0x01000200,
                    bus_address: 0x200,
                    lanes: ByteLanes::Both,
                    kind: AccessKind::Prefetch,
                    function_code: FunctionCode::SupervisorProgram,
                };
                let start = CpuClocks(if failure { 1 } else { 0 });
                let expected = if observed {
                    direct.read_instruction_cycle_observed(
                        access,
                        start,
                        InterruptAperture::FollowingPrefetch,
                    )
                } else {
                    zeff_m68k::timing::ObservedCycle::unobserved(
                        direct.read_instruction_cycle(access, start),
                    )
                };
                let capture = Capture::begin().unwrap();
                let mut adapter = TraceBus::new(&mut traced);
                let actual = if observed {
                    adapter.read_instruction_cycle_observed(
                        access,
                        start,
                        InterruptAperture::FollowingPrefetch,
                    )
                } else {
                    zeff_m68k::timing::ObservedCycle::unobserved(
                        adapter.read_instruction_cycle(access, start),
                    )
                };
                let report = capture.finish(&terminal).unwrap();
                assert_eq!(actual, expected);
                assert_eq!(traced, direct);
                assert_eq!(report.total_callbacks, 1);
                let record = report.history[0].unwrap();
                assert_eq!(record.callback, Callback::Read);
                assert_eq!(record.wait_clocks, 0);
                assert_eq!(report.first_failed_sequence, failure.then_some(0));
                if failure {
                    assert!(actual.cycle.result.is_err());
                    assert_eq!(actual.samples, None);
                } else {
                    let input = actual.cycle.result.unwrap();
                    assert_eq!(input.value, 0x60fe);
                    assert_eq!(
                        input.origin.map(|origin| origin.storage_offset()),
                        [Some(0x200), Some(0x201)]
                    );
                    assert_eq!(record.outcome, Some(Outcome::Read(0x60fe)));
                    assert_eq!(
                        actual.samples.map(|samples| samples.levels()),
                        (observed && !port_history).then_some((4, 4))
                    );
                }
            }
        }
    }
}
