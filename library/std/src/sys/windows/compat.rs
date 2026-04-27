//! A "compatibility layer" for supporting older versions of Windows
//!
//! The standard library uses some Windows API functions that are not present
//! on older versions of Windows.  (Note that the oldest version of Windows
//! that Rust supports is Windows 7 (client) and Windows Server 2008 (server).)
//! This module implements a form of delayed DLL import binding, using
//! `GetModuleHandle` and `GetProcAddress` to look up DLL entry points at
//! runtime.
//!
//! This is implemented simply by storing a function pointer in an atomic.
//! Loading and calling this function will have little or no overhead
//! compared with calling any other dynamically imported function.
//!
//! The stored function pointer starts out as an importer function which will
//! swap itself with the real function when it's called for the first time. If
//! the real function can't be imported then a fallback function is used in its
//! place. While this is low cost for the happy path (where the function is
//! already loaded) it does mean there's some overhead the first time the
//! function is called. In the worst case, multiple threads may all end up
//! importing the same function unnecessarily.

/// Load a function or use a fallback implementation if that fails.
macro_rules! compat_fn {
    ($module:literal: $(
        $(#[$meta:meta])*
        $vis:vis fn $symbol:ident($($argname:ident: $argtype:ty),*) -> $rettype:ty $fallback_body:block
    )*) => ($(
        $(#[$meta])*
        pub mod $symbol {
            #[allow(unused_imports)]
            use super::*;
            use crate::mem;

            type F = unsafe extern "system" fn($($argtype),*) -> $rettype;

            /// Points to the DLL import, or the fallback function.
            ///
            /// This static can be an ordinary, unsynchronized, mutable static because
            /// we guarantee that all of the writes finish during CRT initialization,
            /// and all of the reads occur after CRT initialization.
            static mut PTR: Option<F> = None;

            /// This symbol is what allows the CRT to find the `init` function and call it.
            /// It is marked `#[used]` because otherwise Rust would assume that it was not
            /// used, and would remove it.
            #[used]
            #[link_section = ".CRT$XCU"]
            static INIT_TABLE_ENTRY: unsafe extern "C" fn() = init;

            unsafe extern "C" fn init() {
                PTR = get_f();
            }

            unsafe extern "C" fn get_f() -> Option<F> {
                // There is no locking here. This code is executed before main() is entered, and
                // is guaranteed to be single-threaded.
                //
                // DO NOT do anything interesting or complicated in this function! DO NOT call
                // any Rust functions or CRT functions, if those functions touch any global state,
                // because this function runs during global initialization. For example, DO NOT
                // do any dynamic allocation, don't call LoadLibrary, etc.
                let module_name: *const u8 = concat!($module, "\0").as_ptr();
                let symbol_name: *const u8 = concat!(stringify!($symbol), "\0").as_ptr();
                let module_handle = $crate::sys::c::GetModuleHandleA(module_name as *const i8);
                if !module_handle.is_null() {
                    let ptr = $crate::sys::c::GetProcAddress(module_handle, symbol_name as *const i8);
                    if !ptr.is_null() {
                        // Transmute to the right function pointer type.
                        return Some(mem::transmute(ptr));
                    }
                }
                return None;
            }

            #[allow(dead_code)]
            #[inline(always)]
            pub fn option() -> Option<F> {
                unsafe {
                    if cfg!(miri) {
                        // Miri does not run `init`, so we just call `get_f` each time.
                        get_f()
                    } else {
                        PTR
                    }
                }
            }

            #[allow(dead_code)]
            pub unsafe fn call($($argname: $argtype),*) -> $rettype {
                if let Some(ptr) = option() {
                    return ptr($($argname),*);
                }
                $fallback_body
            }
        }

        $(#[$meta])*
        pub use $symbol::call as $symbol;
    )*)
}
