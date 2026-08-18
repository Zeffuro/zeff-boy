use std::num::NonZeroU64;

pub trait CpuStep {
    /// Returns progress in the CPU's own clock domain.
    fn cpu_cycles(&self) -> Option<NonZeroU64>;
}

impl CpuStep for u64 {
    #[inline]
    fn cpu_cycles(&self) -> Option<NonZeroU64> {
        NonZeroU64::new(*self)
    }
}

impl CpuStep for u32 {
    #[inline]
    fn cpu_cycles(&self) -> Option<NonZeroU64> {
        NonZeroU64::new(u64::from(*self))
    }
}

impl<S: CpuStep> CpuStep for Option<S> {
    #[inline]
    fn cpu_cycles(&self) -> Option<NonZeroU64> {
        self.as_ref()?.cpu_cycles()
    }
}

pub trait CpuCore<B>: Sized {
    type Step: CpuStep;

    /// Executes one architecturally meaningful CPU step.
    fn step_cpu(&mut self, bus: &mut B) -> Self::Step;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_results_report_forward_progress() {
        assert_eq!(4_u32.cpu_cycles().map(NonZeroU64::get), Some(4));
        assert_eq!(7_u64.cpu_cycles().map(NonZeroU64::get), Some(7));
        assert_eq!(Some(3_u32).cpu_cycles().map(NonZeroU64::get), Some(3));
        assert_eq!(None::<u32>.cpu_cycles(), None);
        assert_eq!(0_u64.cpu_cycles(), None);
    }
}
