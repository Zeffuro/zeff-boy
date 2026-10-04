pub(super) const CONTROL_MASK: u32 = 0xffc0;
pub(super) const REQUIRED_CONTROLS: u32 = 0x1f80;

pub(crate) fn floating_point_controls() -> Option<u32> {
    #[cfg(target_arch = "x86_64")]
    {
        let mut controls = 0u32;
        // Read this thread's MXCSR without changing exception status or rounding.
        unsafe {
            std::arch::asm!(
                "stmxcsr [{address}]",
                address = in(reg) &mut controls,
                options(nostack, preserves_flags)
            );
        }
        Some(controls)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

pub(super) fn supported(controls: Option<u32>) -> bool {
    controls.is_some_and(|controls| controls & CONTROL_MASK == REQUIRED_CONTROLS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_bit_is_fenced_but_exception_status_is_ignored() {
        assert!(supported(Some(REQUIRED_CONTROLS)));
        assert!(supported(Some(REQUIRED_CONTROLS | 0x3f)));
        assert!(!supported(None));
        for bit in 6..16 {
            assert!(!supported(Some(REQUIRED_CONTROLS ^ (1 << bit))));
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn control_reader_observes_the_calling_worker() {
        let worker = std::thread::spawn(|| {
            let initial = floating_point_controls().unwrap();
            let altered = (initial & !0x6000) | 0x2000;
            unsafe {
                std::arch::asm!("ldmxcsr [{address}]", address = in(reg) &altered, options(nostack, preserves_flags));
            }
            let observed = floating_point_controls();
            unsafe {
                std::arch::asm!("ldmxcsr [{address}]", address = in(reg) &initial, options(nostack, preserves_flags));
            }
            observed
        });
        assert!(!supported(worker.join().unwrap()));
    }
}
