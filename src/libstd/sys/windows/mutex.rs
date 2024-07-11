//! System Mutexes
//!
//! The Windows implementation of mutexes is a little odd and it may not be
//! immediately obvious what's going on. The primary oddness is that SRWLock is
//! used instead of CriticalSection, and this is done because:
//!
//! 1. SRWLock is several times faster than CriticalSection according to
//!    benchmarks performed on both Windows 8 and Windows 7.
//!
//! 2. CriticalSection allows recursive locking while SRWLock deadlocks. The
//!    Unix implementation deadlocks so consistency is preferred. See #19962 for
//!    more details.
//!
//! 3. While CriticalSection is fair and SRWLock is not, the current Rust policy
//!    is that there are no guarantees of fairness.
//!
//! The downside of this approach, however, is that SRWLock is not available on
//! Windows XP, so we continue to have a fallback implementation where
//! CriticalSection is used and we keep track of who's holding the mutex to
//! detect recursive locks.

use crate::cell::UnsafeCell;
use crate::mem::{MaybeUninit};
use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sys::c;

pub struct Mutex {
    lock: AtomicUsize,
    held: UnsafeCell<bool>,
}

unsafe impl Send for Mutex {}
unsafe impl Sync for Mutex {}

impl Mutex {
    pub const fn new() -> Mutex {
        Mutex {
            // This works because SRWLOCK_INIT is 0 (wrapped in a struct), so we are also properly
            // initializing an SRWLOCK here.
            lock: AtomicUsize::new(0),
            held: UnsafeCell::new(false),
        }
    }
    #[inline]
    pub unsafe fn init(&mut self) {}
    pub unsafe fn lock(&self) {
                let re = self.remutex();
                (*re).lock();
                if !self.flag_locked() {
                    (*re).unlock();
                    panic!("cannot recursively lock a mutex");
                }
    }
    pub unsafe fn try_lock(&self) -> bool {
                let re = self.remutex();
                if !(*re).try_lock() {
                    false
                } else if self.flag_locked() {
                    true
                } else {
                    (*re).unlock();
                    false
                }
    }
    pub unsafe fn unlock(&self) {
        *self.held.get() = false;
            (*self.remutex()).unlock()
    }
    pub unsafe fn destroy(&self) {
            match self.lock.load(Ordering::SeqCst) {
                0 => {}
                n => {
                    Box::from_raw(n as *mut ReentrantMutex).destroy();
                }
            }
    }

    unsafe fn remutex(&self) -> *mut ReentrantMutex {
        match self.lock.load(Ordering::SeqCst) {
            0 => {}
            n => return n as *mut _,
        }
        let re = box ReentrantMutex::uninitialized();
        re.init();
        let re = Box::into_raw(re);
        match self.lock.compare_and_swap(0, re as usize, Ordering::SeqCst) {
            0 => re,
            n => {
                Box::from_raw(re).destroy();
                n as *mut _
            }
        }
    }

    unsafe fn flag_locked(&self) -> bool {
        if *self.held.get() {
            false
        } else {
            *self.held.get() = true;
            true
        }
    }
}

pub struct ReentrantMutex {
    inner: UnsafeCell<MaybeUninit<c::CRITICAL_SECTION>>,
}

unsafe impl Send for ReentrantMutex {}
unsafe impl Sync for ReentrantMutex {}

impl ReentrantMutex {
    pub const fn uninitialized() -> ReentrantMutex {
        ReentrantMutex { inner: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    pub unsafe fn init(&self) {
        c::InitializeCriticalSectionAndSpinCount((&mut *self.inner.get()).as_mut_ptr(), 2000);
    }

    pub unsafe fn lock(&self) {
        c::EnterCriticalSection((&mut *self.inner.get()).as_mut_ptr());
    }

    #[inline]
    pub unsafe fn try_lock(&self) -> bool {
        c::TryEnterCriticalSection((&mut *self.inner.get()).as_mut_ptr()) != 0
    }

    pub unsafe fn unlock(&self) {
        c::LeaveCriticalSection((&mut *self.inner.get()).as_mut_ptr());
    }

    pub unsafe fn destroy(&self) {
        c::DeleteCriticalSection((&mut *self.inner.get()).as_mut_ptr());
    }
}
