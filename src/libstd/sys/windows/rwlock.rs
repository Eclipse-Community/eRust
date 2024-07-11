use crate::cell::UnsafeCell;
use crate::sys::mutex::ReentrantMutex;
use crate::sync::atomic::{AtomicUsize, Ordering};

pub struct RWLock {
    lock: AtomicUsize,
    held: UnsafeCell<bool>,
}

unsafe impl Send for RWLock {}
unsafe impl Sync for RWLock {}

impl RWLock {
    pub const fn new() -> RWLock {
        RWLock {
            lock: AtomicUsize::new(0),
            held: UnsafeCell::new(false),
            }
    }
    #[inline]
    pub unsafe fn read(&self) {
                let re = self.remutex();
                (*re).lock();
                if !self.flag_locked() {
                    (*re).unlock();
                    panic!("cannot recursively lock a mutex");
                }
    }
    #[inline]
    pub unsafe fn try_read(&self) -> bool {
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
    #[inline]
    pub unsafe fn write(&self) {
                RWLock::read(&self);
    }
    #[inline]
    pub unsafe fn try_write(&self) -> bool {
                RWLock::try_read(&self)
    }
    #[inline]
    pub unsafe fn read_unlock(&self) {
        *self.held.get() = false;
        (*self.remutex()).unlock();
    }
    #[inline]
    pub unsafe fn write_unlock(&self) {
        RWLock::read_unlock(&self)
    }

    #[inline]
    pub unsafe fn destroy(&self) {
        match self.lock.load(Ordering::SeqCst) {
            0 => {}
            n => { Box::from_raw(n as *mut ReentrantMutex).destroy(); }
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
            n => { Box::from_raw(re).destroy(); n as *mut _ }
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
