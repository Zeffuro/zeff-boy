use super::*;
use crate::debug::{DebugUiActions, md_timed_ports::MdTimedPortUi};
use crate::emu_thread::MdTimedPortRequest;
use zeff_md_core::emulator::DebugPortWrite;

#[wasm_bindgen_test]
async fn md_timed_ports_browser_pending_save_keeps_queue_reply_and_deferred_actions() {
    clear().await;
    let mut backend = load(512, 0).unwrap();
    backend.debug_suspend();
    let thread = EmuThread::spawn(backend, false);
    let request = MdTimedPortRequest {
        id: 811,
        writes: vec![DebugPortWrite::Word {
            address: 0xc00004,
            value: 0x8f02,
        }],
    };
    let mut state = MdTimedPortUi::default();
    state.queue(request.clone()).unwrap();
    let mut actions = DebugUiActions::none();
    actions.memory_writes.push((0xff0041, 0xa5));
    state.defer(actions);
    thread.send(EmuCommand::SaveStateSlot(7));
    assert!(thread.inner.borrow().pending_storage.is_some());
    assert!(thread.recv().is_none());
    assert_eq!(state.queued().unwrap().id, 811);
    assert!(state.has_deferred());
    let input = frame_input();
    let input = crate::app::md_timed_ports::isolated_input(request, input.snapshot, input.buffers);
    state.sent(811);
    thread.send(EmuCommand::StepFrames(Box::new(input)));
    let frame = thread.try_recv_frame().unwrap();
    assert!(frame.runtime_fault.is_none());
    assert_eq!(
        (frame.advanced_frames, frame.completed_step_requests),
        (0, 1)
    );
    let Some(EmuResponse::MdTimedPortsFinished(reply)) = thread.recv() else {
        panic!("timed reply was lost behind pending storage");
    };
    assert_eq!(reply.id, 811);
    assert_eq!(reply.result.as_ref().unwrap().instructions, 0);
    assert!(state.receive(reply));
    assert!(state.ordinary_flush_required() && state.has_deferred());
    assert!(thread.recv().is_none());
    assert!(state.ordinary_flush_required() && state.has_deferred());
    let deadline = js_sys::Date::now() + 10_000.0;
    loop {
        match thread.recv() {
            Some(EmuResponse::SaveStateOk { .. }) => break,
            Some(_) => panic!("unrelated response replaced asynchronous save completion"),
            None => {}
        }
        assert!(js_sys::Date::now() < deadline);
        browser_test_delay(5).await.unwrap();
    }
    assert!(thread.inner.borrow().pending_storage.is_none());
    assert!(state.ordinary_flush_required() && state.has_deferred());
    assert!(state.take_deferred().is_none());
    state.ordinary_flush_sent();
    assert!(state.take_deferred().is_none());
    state.ordinary_delivery();
    assert_eq!(
        state.take_deferred().unwrap().memory_writes,
        [(0xff0041, 0xa5)]
    );
    drop(thread);
    clear().await;
}
