use super::*;

fn input(fill: bool) -> PrimitiveInputs {
    PrimitiveInputs {
        phases: PhaseLevels {
            data_master: false,
            data_slave: false,
            horizontal_master: true,
            horizontal_slave: false,
        },
        main: MainRequests {
            write: false,
            read: false,
            control_select: false,
            data_bus: 0,
        },
        frontend: FrontendFeedback {
            second_word_ready: true,
            first_word_ready: true,
            dma_bus_ready: true,
            read_cycle_ready: true,
            external_address_cycle: false,
            cpu_data_capture: false,
        },
        memory: MemoryFeedback {
            read_selected: !fill,
            write_selected: false,
            fill_selected: fill,
            external_counter_cycle: false,
            dma_active: true,
        },
        reset_comb: false,
        double_counter_test: false,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_local_service_acceptance_ignores_held_master_settling() {
    for fill in [false, true] {
        let mut core = ControlCore::software_origin();
        core.registers.dma.control = if fill { 0x80 } else { 0xc0 };
        core.registers.dma.length = [0xfffc; 2];
        let mut sample = input(fill);
        assert!(core.evaluate(sample).local_dma_service);
        assert_eq!(core.registers.dma.length_banks(), [0xfffd, 0xfffc]);
        for _ in 0..8 {
            assert!(!core.evaluate(sample).local_dma_service);
            assert_eq!(core.registers.dma.length_banks(), [0xfffd, 0xfffc]);
        }
        sample.phases.horizontal_master = false;
        sample.phases.horizontal_slave = true;
        assert!(!core.evaluate(sample).local_dma_service);
        assert_eq!(core.registers.dma.length_banks(), [0xfffd; 2]);
        sample.phases.horizontal_master = true;
        sample.phases.horizontal_slave = false;
        assert!(core.evaluate(sample).local_dma_service);
        assert_eq!(core.registers.dma.length_banks(), [0xfffe, 0xfffd]);
    }
    let mut core = ControlCore::software_origin();
    let mut ordinary = input(false);
    ordinary.memory.dma_active = false;
    ordinary.memory.external_counter_cycle = true;
    assert!(!core.evaluate(ordinary).local_dma_service);
    assert_eq!(core.registers.dma.length_banks(), [0, 0xffff]);
}
