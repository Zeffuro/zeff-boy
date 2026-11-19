use super::*;

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
let originalTransaction = null;
let abortCount = 0;
export function md_arm_transaction_abort() {
    if (originalTransaction) throw new Error('transaction abort already armed');
    originalTransaction = IDBDatabase.prototype.transaction;
    abortCount = 0;
    IDBDatabase.prototype.transaction = function(...args) {
        const tx = originalTransaction.apply(this, args);
        if (this.name === 'zeff-boy-saves' && args[1] === 'readwrite') {
            md_restore_transaction();
            queueMicrotask(() => { abortCount++; tx.abort(); });
        }
        return tx;
    };
}
export function md_restore_transaction() {
    if (originalTransaction) {
        IDBDatabase.prototype.transaction = originalTransaction;
        originalTransaction = null;
    }
}
export function md_transaction_abort_count() { return abortCount; }
")]
extern "C" {
    fn md_arm_transaction_abort();
    fn md_restore_transaction();
    fn md_transaction_abort_count() -> u32;
}

pub(super) struct AbortNextWrite;

impl AbortNextWrite {
    pub(super) fn arm() -> Self {
        md_arm_transaction_abort();
        Self
    }

    pub(super) fn count(&self) -> u32 {
        md_transaction_abort_count()
    }
}

impl Drop for AbortNextWrite {
    fn drop(&mut self) {
        md_restore_transaction();
    }
}

pub(super) async fn wait_for_failed_shutdown(thread: &EmuThread) {
    let deadline = js_sys::Date::now() + 10_000.0;
    let mut responses = Vec::new();
    loop {
        while let Some(response) = thread.try_recv_response() {
            match response {
                EmuResponse::SramFlushFailed(error) => {
                    assert!(error.contains("IndexedDB transaction failed"));
                    responses.push("sram-failed");
                }
                EmuResponse::RecoverySaveFailed(error) => {
                    assert_eq!(error, "browser recovery transaction did not commit");
                    responses.push("recovery-failed");
                }
                EmuResponse::ShutdownComplete => {
                    responses.push("shutdown");
                    assert_eq!(responses, ["sram-failed", "recovery-failed", "shutdown"]);
                    assert!(thread.try_recv_response().is_none());
                    return;
                }
                _ => panic!("unexpected response during failed browser shutdown"),
            }
        }
        assert!(
            js_sys::Date::now() < deadline,
            "failed browser shutdown timed out"
        );
        browser_test_delay(5).await.unwrap();
    }
}
