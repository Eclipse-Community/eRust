#![cfg_attr(test, allow(dead_code))]

use crate::sys::c;
use crate::thread;

pub struct Handler;

impl Handler {
    pub unsafe fn new() -> Handler {
        Handler
    }
}

extern "system" fn vectored_handler(ExceptionInfo: *mut c::EXCEPTION_POINTERS) -> c::LONG {
    unsafe {
        let rec = &(*(*ExceptionInfo).ExceptionRecord);
        let code = rec.ExceptionCode;

        if code == c::EXCEPTION_STACK_OVERFLOW {
            rtprintpanic!(
                "\nthread '{}' has overflowed its stack\n",
                thread::current().name().unwrap_or("<unknown>")
            );
        }
        c::EXCEPTION_CONTINUE_SEARCH
    }
}

pub unsafe fn init() {
    if c::AddVectoredExceptionHandler(0, vectored_handler).is_null() {
        panic!("failed to install exception handler");
    }
    // Set the thread stack guarantee for the main thread.
    let _h = Handler::new();
}
