//! The phone's door into the core (WebAssembly only): the phone writes a JSON request into
//! memory it got from `mk_alloc`, calls `mk_call`, and reads back a length-prefixed JSON answer
//! (4 bytes little-endian length, then the bytes), which it then frees with `mk_free`.
//! No generated bindings are needed; see `api.rs` for the functions.

#[no_mangle]
pub extern "C" fn mk_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Frees memory from `mk_alloc` or an answer from `mk_call` (`len` = the full allocation).
///
/// # Safety
/// `ptr`/`len` must be exactly what `mk_alloc` returned or an answer's 4 + length bytes.
#[no_mangle]
pub unsafe extern "C" fn mk_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

/// Runs a request (`len` bytes of UTF-8 JSON at `ptr`, which is freed) and returns the answer.
///
/// # Safety
/// `ptr` must come from `mk_alloc(len)` with `len` bytes written.
#[no_mangle]
pub unsafe extern "C" fn mk_call(ptr: *mut u8, len: usize) -> *mut u8 {
    let input = Vec::from_raw_parts(ptr, len, len.max(1));
    let answer = crate::api::call(&String::from_utf8_lossy(&input));
    let mut out = Vec::with_capacity(4 + answer.len());
    out.extend_from_slice(&(answer.len() as u32).to_le_bytes());
    out.extend_from_slice(answer.as_bytes());
    let mut out = out.into_boxed_slice();
    let p = out.as_mut_ptr();
    std::mem::forget(out);
    p
}
