cfg_select! {
    any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "motor",
        target_os = "dragonfly",
        all(target_family = "wasm", target_feature = "atomics"),
        target_os = "hermit",
    ) => {
        mod futex;
        pub use futex::Mutex;
    }
    target_os = "fuchsia" => {
        mod fuchsia;
        pub use fuchsia::Mutex;
    }
    any(
        target_family = "unix",
        target_os = "teeos",
    ) => {
        mod pthread;
        pub use pthread::Mutex;
    }
    target_os = "windows" => {
        mod windows7;
        pub use windows7::{Mutex, raw};
    }
    all(target_vendor = "fortanix", target_env = "sgx") => {
        mod sgx;
        pub use sgx::Mutex;
    }
    target_os = "solid_asp3" => {
        mod itron;
        pub use itron::Mutex;
    }
    target_os = "xous" => {
        mod xous;
        pub use xous::Mutex;
    }
    _ => {
        mod no_threads;
        pub use no_threads::Mutex;
    }
}
