use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_copy_accepts_read_services_without_recounting_final_write() {
    for concurrent in [false, true] {
        let mut machine = copy_machine(concurrent);
        let mut epoch = machine.debug_dma_activity_epoch();
        for (at, accepted, length, bytes) in [
            (1029, true, 2, [0, 0]),
            (1057, true, 1, [0, 0x34]),
            (1085, false, 0, [0x12, 0x34]),
        ] {
            machine.step_instruction().unwrap();
            assert_eq!(machine.bus.now.get(), at);
            assert_eq!(machine.debug_dma_activity_epoch() != epoch, accepted);
            epoch = machine.debug_dma_activity_epoch();
            assert_eq!(machine.vdp().registers().registers()[19], Some(length));
            assert_eq!(&machine.vdp().memory().vram()[0x100..0x102], &bytes);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_dma_forecasts_and_failed_capture_do_not_publish_activity() {
    for concurrent in [false, true] {
        let mut machine = fill_machine(concurrent, 0x2000);
        let before = machine.clone();
        let epoch = machine.debug_dma_activity_epoch();
        assert!(machine.bus.vdp.capture_line_at(tick(1330), REGION).is_err());
        assert_eq!(machine, before);
        assert_eq!(machine.debug_dma_activity_epoch(), epoch);
        machine.bus.advance_machine(370).unwrap();
        assert_ne!(machine.debug_dma_activity_epoch(), epoch);
        assert_eq!(machine.vdp().memory().vram()[0x2001], 0xab);

        let mut machine = super::super::external_dma::fixture(concurrent, false);
        let before = machine.clone();
        let epoch = machine.debug_dma_activity_epoch();
        let release = machine
            .vdp()
            .external_dma_release(
                REGION,
                crate::VdpDmaSource::from_cartridge(&machine.bus.cartridge, &machine.bus.ram),
            )
            .unwrap()
            .unwrap();
        assert_eq!(machine, before);
        assert_eq!(machine.debug_dma_activity_epoch(), epoch);
        machine
            .bus
            .advance_machine(release.get() - machine.bus.now.get())
            .unwrap();
        assert_ne!(machine.debug_dma_activity_epoch(), epoch);
        let tail_epoch = machine.debug_dma_activity_epoch();
        machine.bus.advance_machine(2000).unwrap();
        assert_eq!(machine.debug_dma_activity_epoch(), tail_epoch);
        assert_eq!(&machine.vdp().memory().vram()[0x400..0x422], &[0x44; 34]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_fill_waiting_and_cpu_seed_retirement_are_not_dma_services() {
    let mut machine = super::super::machine(&[NEXT; 8]);
    machine.bus.vdp = Vdp::bootstrap_mode5();
    machine.bus.now = MasterTicks::ZERO;
    machine.bus.audio = crate::hardware::audio::Audio::default();
    configure(&mut machine, 2, 0x80, 1);
    machine.bus.vdp.write_control(0x8114, REGION).unwrap();
    machine.bus.advance_machine(100).unwrap();
    let epoch = machine.debug_dma_activity_epoch();
    machine.bus.vdp.write_control(0x4400, REGION).unwrap();
    machine.bus.vdp.write_control(0x0080, REGION).unwrap();
    machine.bus.advance_machine(1000).unwrap();
    assert_eq!(machine.debug_dma_activity_epoch(), epoch);
    machine.bus.vdp.write_data(0, REGION).unwrap();
    for _ in 0..2048 {
        if machine.vdp().fifo().is_empty() {
            break;
        }
        machine.bus.advance_machine(1).unwrap();
        assert_eq!(machine.debug_dma_activity_epoch(), epoch);
    }
    assert!(machine.vdp().fifo().is_empty());
    for _ in 0..2048 {
        machine.bus.advance_machine(1).unwrap();
        if machine.debug_dma_activity_epoch() != epoch {
            break;
        }
    }
    assert_ne!(machine.debug_dma_activity_epoch(), epoch);
    assert_eq!(machine.vdp().memory().vram()[0x400], 0);
    assert_eq!(machine.vdp().memory().vram()[0x401], 0);
}
