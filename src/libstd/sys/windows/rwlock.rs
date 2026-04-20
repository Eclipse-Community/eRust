use crate::cell::UnsafeCell;
use crate::mem::{MaybeUninit};
use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sys::c;

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
                /*let re = self.remutex();
                (*re).lock();
                if !self.flag_locked() {
                    (*re).unlock();
                    panic!("cannot recursively lock a mutex");
                }*/
    }
    #[inline]
    pub unsafe fn try_write(&self) -> bool {
                RWLock::try_read(&self)
                /*let re = self.remutex();
                if !(*re).try_lock() {
                    false
                } else if self.flag_locked() {
                    true
                } else {
                    (*re).unlock();
                    false
                }*/
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
        let mut re = box ReentrantMutex::uninitialized();
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
            
pub struct ReentrantMutex { inner: UnsafeCell<MaybeUninit<c::CRITICAL_SECTION>> }

unsafe impl Send for ReentrantMutex {}
unsafe impl Sync for ReentrantMutex {}

impl ReentrantMutex {
    pub fn uninitialized() -> ReentrantMutex {
        ReentrantMutex { inner: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    pub unsafe fn init(&mut self) {
        c::InitializeCriticalSection((&mut *self.inner.get()).as_mut_ptr());
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
