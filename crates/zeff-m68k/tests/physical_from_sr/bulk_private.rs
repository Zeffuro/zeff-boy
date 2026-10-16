use super::*;
use bulk_oracle::Expected;

pub(super) fn verify(cpu: &PrefetchCpu, e: &Expected) -> bool {
    let words = e.queue.unwrap_or([0, 0]);
    let Some(image) = PrefetchCpu::from_prefetched(e.r, words, CpuClocks(e.clock)) else {
        return false;
    };
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
