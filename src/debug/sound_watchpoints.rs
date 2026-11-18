use crate::ui::TraceStreamId;
use zeff_emu_common::debug::WatchType;
use zeff_md_core::sound_trace::{SoundWatchHit, SoundWatchRule, SoundWatchSpace, SoundWatchpoint};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SoundWatchBinding {
    pub(crate) owner: TraceStreamId,
    pub(crate) branch: TraceStreamId,
    pub(crate) policy: TraceStreamId,
    pub(crate) revision: u64,
}
#[derive(Clone, Copy)]
pub(crate) struct SoundWatchRequest {
    pub(crate) binding: SoundWatchBinding,
    pub(crate) request: TraceStreamId,
    pub(crate) rule: SoundWatchRule,
    pub(crate) add: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DelayedSoundWatchHit {
    pub(crate) observed: SoundWatchHit,
    pub(crate) main_pc: u32,
    pub(crate) master_tick: u64,
    pub(crate) faulted: bool,
}
pub(crate) struct SoundWatchView {
    pub(crate) binding: Option<SoundWatchBinding>,
    pub(crate) watches: Vec<SoundWatchpoint>,
    pub(crate) hit: Option<DelayedSoundWatchHit>,
    pub(crate) gaps: u64,
    pub(crate) blocked: bool,
    pub(crate) notice: Option<&'static str>,
    pub(crate) rejection: Option<&'static str>,
}
#[derive(Clone, Default)]
struct Form {
    start: String,
    end: String,
    io: bool,
    write: bool,
    read: bool,
}
fn request(
    actions: &mut super::DebugUiActions,
    binding: SoundWatchBinding,
    rule: SoundWatchRule,
    add: bool,
) {
    actions.sound_trace.watch = Some(SoundWatchRequest {
        binding,
        rule,
        add,
        request: TraceStreamId::allocate(),
    });
}
pub(crate) fn draw(ui: &mut egui::Ui, view: &SoundWatchView, actions: &mut super::DebugUiActions) {
    ui.separator();
    ui.strong("SOUND operand / stack and I/O watches");
    ui.weak("Stops after the enclosing MAIN boundary. Fetch, HALT polls and interrupt acknowledge are excluded.");
    if let Some(reason) = view.rejection {
        ui.label(reason);
        return;
    }
    if let Some(notice) = view.notice {
        ui.weak(notice);
    }
    if view.blocked {
        ui.label(
            "Observation overflowed; execution stopped. Remove and re-add a watch to continue observing.",
        );
    }
    if view.gaps != 0 {
        ui.label(format!("Observation gaps: {}", view.gaps));
    }
    if let Some(hit) = view.hit {
        let a = hit.observed.access;
        ui.label(format!("First accepted {:?} {:?} {:04X} = {:02X}; SOUND PC {:04X}, segment-end cycle {}, access #{}",
            a.space, a.kind, a.address, a.value, hit.observed.pc, hit.observed.cycle, hit.observed.ordinal));
        ui.label(format!(
            "Delayed MAIN stop PC {:06X}, master tick {}{}",
            hit.main_pc,
            hit.master_tick,
            if hit.faulted {
                " (faulted boundary)"
            } else {
                ""
            }
        ));
        ui.weak("Resume through MAIN controls.");
    }
    let Some(binding) = view.binding else {
        return;
    };
    let id = ui.id().with("sound-watch-form");
    let mut form = ui
        .data_mut(|data| data.get_temp::<Form>(id))
        .unwrap_or_else(|| Form {
            read: true,
            write: true,
            ..Default::default()
        });
    ui.horizontal(|ui| {
        ui.checkbox(&mut form.io, "I/O (00–FF)");
        ui.checkbox(&mut form.read, "Read");
        ui.checkbox(&mut form.write, "Write");
    });
    ui.horizontal(|ui| {
        ui.label("Hex range");
        ui.add(egui::TextEdit::singleline(&mut form.start).desired_width(58.0));
        ui.label("to");
        ui.add(egui::TextEdit::singleline(&mut form.end).desired_width(58.0));
        let range = u16::from_str_radix(form.start.trim(), 16)
            .ok()
            .zip(u16::from_str_radix(form.end.trim(), 16).ok());
        let kind = match (form.read, form.write) {
            (true, true) => Some(WatchType::ReadWrite),
            (true, false) => Some(WatchType::Read),
            (false, true) => Some(WatchType::Write),
            _ => None,
        };
        let rule = range
            .zip(kind)
            .filter(|&((start, end), _)| start <= end && (!form.io || end <= 255))
            .map(|((start, end), kind)| SoundWatchRule {
                space: if form.io {
                    SoundWatchSpace::Io
                } else {
                    SoundWatchSpace::Memory
                },
                start,
                end,
                kind,
            });
        if ui
            .add_enabled(rule.is_some(), egui::Button::new("Add watch"))
            .clicked()
        {
            request(actions, binding, rule.unwrap(), true);
        }
    });
    ui.data_mut(|data| data.insert_temp(id, form));
    for watch in &view.watches {
        ui.horizontal(|ui| {
            let r = watch.rule;
            ui.label(format!(
                "{:?} {:04X}–{:04X} {:?}: {} accepted hits",
                r.space, r.start, r.end, r.kind, watch.hits
            ));
            if ui.small_button("Remove").clicked() {
                request(actions, binding, r, false);
            }
        });
    }
}
