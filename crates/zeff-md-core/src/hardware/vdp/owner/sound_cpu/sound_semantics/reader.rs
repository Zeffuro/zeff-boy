use super::*;
use std::cell::{Cell, RefCell};

pub(super) struct Run {
    pub next: Cpu,
    pub state: BusState,
    pub instruction: FetchedInstruction,
    pub accesses: Vec<Access>,
    pub complete: bool,
    pub consumed: Option<(u16, u8)>,
    pub input_lost: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    cpu: &Cpu,
    state: BusState,
    memory: &SoundMemory,
    audio: Option<&Audio>,
    tick: u64,
    irq: bool,
    sample: Option<(u16, u8)>,
    steps: u8,
) -> Result<(Cpu, BusState, FetchedInstruction, Vec<Access>)> {
    let run = run_capturing::<false>(
        cpu,
        state,
        ReadContext {
            memory,
            audio,
            tick,
            irq,
        },
        sample,
        steps,
    )?;
    Ok((run.next, run.state, run.instruction, run.accesses))
}

pub(super) struct ReadContext<'a> {
    pub memory: &'a SoundMemory,
    pub audio: Option<&'a Audio>,
    pub tick: u64,
    pub irq: bool,
}

struct Recorder<'a, const CAPTURE: bool> {
    input: Cell<Option<(u16, u8)>>,
    input_lost: Cell<bool>,
    memory: &'a SoundMemory,
    audio: Option<&'a Audio>,
    tick: u64,
    irq: bool,
    sample: Option<(u16, u8)>,
    accesses: RefCell<Vec<Access>>,
    invalid: Cell<bool>,
}

impl<const CAPTURE: bool> Z80Bus for Recorder<'_, CAPTURE> {
    fn cpu_read_instruction(&self, address: u16) -> u8 {
        let value = self.cpu_read(address);
        if CAPTURE {
            self.input_lost
                .set(self.input_lost.get() || self.input.replace(Some((address, value))).is_some());
        }
        value
    }
    fn cpu_read(&self, address: u16) -> u8 {
        let value = if let Some((expected, value)) = self.sample {
            if expected != address {
                self.invalid.set(true);
            }
            value
        } else if external(address) {
            0
        } else {
            match self.audio.map_or_else(
                || Err(anyhow::anyhow!("unexpected uncaptured local read")),
                |audio| local_byte(self.memory, audio, self.tick, address),
            ) {
                Ok(value) => value,
                Err(_) => {
                    self.invalid.set(true);
                    0
                }
            }
        };
        self.accesses.borrow_mut().push(Access {
            address,
            value,
            write: false,
        });
        value
    }
    fn cpu_write(&mut self, address: u16, value: u8) {
        self.accesses.borrow_mut().push(Access {
            address,
            value,
            write: true,
        });
    }
    fn io_read(&mut self, _port: u8) -> u8 {
        self.invalid.set(true);
        0
    }
    fn io_write(&mut self, _port: u8, _value: u8) {
        self.invalid.set(true);
    }
    fn maskable_interrupt_pending(&self) -> bool {
        self.irq
    }
    fn non_maskable_interrupt_pending(&self) -> bool {
        false
    }
    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        self.invalid.set(true);
        false
    }
    fn acknowledge_maskable_interrupt(&mut self) -> u8 {
        self.invalid.set(true);
        0
    }
}

pub(super) fn run_capturing<const CAPTURE: bool>(
    cpu: &Cpu,
    state: BusState,
    context: ReadContext<'_>,
    sample: Option<(u16, u8)>,
    steps: u8,
) -> Result<Run> {
    let ReadContext {
        memory,
        audio,
        tick,
        irq,
    } = context;
    let mut next = cpu.clone();
    let mut state = state;
    let mut recorder = Recorder::<CAPTURE> {
        input: Cell::new(None),
        input_lost: Cell::new(false),
        memory,
        audio,
        tick,
        irq,
        sample,
        accesses: RefCell::new(Vec::new()),
        invalid: Cell::new(false),
    };
    let mut instruction = None;
    let mut complete = false;
    let mut cycles = 0u32;
    for _ in 0..steps {
        let step = next
            .step_bus_segment(&mut recorder, &mut state)
            .ok_or_else(|| anyhow::anyhow!("suspended SOUND CPU"))?;
        cycles = cycles
            .checked_add(step.instruction.cycles)
            .ok_or_else(|| anyhow::anyhow!("SOUND segment overflow"))?;
        instruction.get_or_insert(step.instruction);
        complete = step.complete;
    }
    ensure!(
        !recorder.invalid.get() && !next.last_step_was_interrupt(),
        "unqualified SOUND I/O or interrupt envelope"
    );
    ensure!(next.trap().is_none(), "SOUND CPU trap: {:?}", next.trap());
    let mut instruction = instruction.ok_or_else(|| anyhow::anyhow!("empty SOUND segment"))?;
    instruction.cycles = cycles;
    ensure!(
        cpu.cycles().checked_add(u64::from(cycles)) == Some(next.cycles()),
        "SOUND semantic cycle overflow"
    );
    let input_lost = recorder.input_lost.get();
    let consumed = if input_lost {
        None
    } else {
        recorder.input.get()
    };
    let accesses = recorder.accesses.into_inner();
    ensure!(
        accesses.len() <= 1,
        "aggregate SOUND segment has multiple memory callbacks"
    );
    Ok(Run {
        next,
        state,
        instruction,
        accesses,
        complete,
        consumed,
        input_lost,
    })
}
