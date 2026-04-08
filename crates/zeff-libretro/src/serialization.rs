use crate::callbacks::*;
use std::os::raw::c_void;
use std::panic::catch_unwind;

const SERIALIZE_LENGTH_PREFIX: usize = 4;

#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize_size() -> usize {
    catch_unwind(|| {
        let core = lock(&CORE);
        let actual = core
            .as_ref()
            .and_then(|s| s.encode_state().ok())
            .map_or(0, |v| v.len());
        let mut max = lock(&MAX_SERIALIZE_SIZE);
        if actual > *max {
            *max = actual;
        }
        SERIALIZE_LENGTH_PREFIX + *max + 4096
    })
    .unwrap_or(0)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    if data.is_null() {
        return false;
    }
    catch_unwind(|| {
        let core = lock(&CORE);
        let Some(state) = core.as_ref() else {
            return false;
        };
        match state.encode_state() {
            Ok(bytes) if SERIALIZE_LENGTH_PREFIX + bytes.len() <= size => {
                unsafe {
                    std::ptr::write_bytes(data as *mut u8, 0, size);
                    let len_bytes = (bytes.len() as u32).to_le_bytes();
                    std::ptr::copy_nonoverlapping(
                        len_bytes.as_ptr(),
                        data as *mut u8,
                        SERIALIZE_LENGTH_PREFIX,
                    );
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr(),
                        (data as *mut u8).add(SERIALIZE_LENGTH_PREFIX),
                        bytes.len(),
                    );
                }
                true
            }
            _ => false,
        }
    })
    .unwrap_or(false)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if data.is_null() || size < SERIALIZE_LENGTH_PREFIX {
        return false;
    }
    catch_unwind(|| {
        let mut core = lock(&CORE);
        let Some(state) = core.as_mut() else {
            return false;
        };
        let all_bytes = unsafe { std::slice::from_raw_parts(data as *const u8, size) };
        let Ok(len_bytes) = all_bytes[..4].try_into() else {
            return false;
        };
        let payload_len = u32::from_le_bytes(len_bytes) as usize;
        if payload_len == 0 || SERIALIZE_LENGTH_PREFIX + payload_len > size {
            return false;
        }
        let payload = &all_bytes[SERIALIZE_LENGTH_PREFIX..SERIALIZE_LENGTH_PREFIX + payload_len];
        state.load_state(payload).is_ok()
    })
    .unwrap_or(false)
}
