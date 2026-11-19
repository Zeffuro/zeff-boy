use super::*;
use bulk_oracle::Expected;

pub(super) fn verify(cpu: &PrefetchCpu, e: &Expected) -> bool {
    let words = e.queue.unwrap_or([0, 0]);
    let Some(mut image) = PrefetchCpu::from_prefetched(e.r, words, CpuClocks(e.clock)) else {
        return false;
    };
    set_interrupts(&mut image, e.level, e.edge);
    let mut text = format!("{image:?}");
    let original = format!(
        "pipeline: Some(Pipeline {{ address: {}, words: {words:?} }}),",
        e.r.pc
    );
    let pipeline = if let Some(queue) = e.queue {
        format!(
            "pipeline: Some(Pipeline {{ address: {}, words: {queue:?} }}),",
            e.r.pc
        )
    } else {
        "pipeline: None,".to_owned()
    };
    for (old, new) in [
        (original, pipeline),
        (
            "frozen: false,".to_owned(),
            format!("frozen: {},", e.guard == Some(TimingFault::Frozen)),
        ),
        (
            "physical_state: Idle,".to_owned(),
            format!("physical_state: {:?},", e.state),
        ),
        (
            "physical_inspection: DiagnosticCompleteValues,".to_owned(),
            format!("physical_inspection: {:?},", e.inspection),
        ),
    ] {
        if text.matches(&old).count() != 1 {
            return false;
        }
        text = text.replacen(&old, &new, 1);
    }
    let good = format!("{cpu:?}") == text;
    if e.guard.is_none()
        && e.state == PhysicalEntryState::Idle
        && matches!(e.result, Ok(PhysicalInstructionStep::Instruction(_)))
    {
        good && *cpu == image
    } else {
        good
    }
}

pub(super) fn set_interrupts(cpu: &mut PrefetchCpu, level: u8, edge: bool) {
    if edge {
        assert!(cpu.interrupt_input().set_level(7));
    }
    assert!(cpu.interrupt_input().set_level(level));
}
pub(super) fn interrupts(cpu: &mut PrefetchCpu, level: u8, edge: bool) -> bool {
    let mut image = PrefetchCpu::default();
    set_interrupts(&mut image, level, edge);
    *cpu.interrupt_input() == *image.interrupt_input()
}
