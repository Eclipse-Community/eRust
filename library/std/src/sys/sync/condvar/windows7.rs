use core::ptr;
use crate::cell::{Cell, UnsafeCell};
use crate::sys::sync::{Mutex, mutex};
use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::mem::{self, MaybeUninit};
use crate::sys::{c, os};
use crate::time::Duration;

pub struct Condvar {
    // This is either directly an CONDITION_VARIABLE (if supported), or a Box<Inner> otherwise.
    inner: AtomicUsize,
}

unsafe impl Send for Condvar {}
unsafe impl Sync for Condvar {}

const SIGNAL: usize = 0;
const BROADCAST: usize = 1;
const MAX_EVENTS: usize = 2;

struct Inner {
    waiting: Cell<u32> ,
    lock_waiting: ReentrantMutex ,
    events: [UnsafeCell<c::HANDLE>; MAX_EVENTS],
    broadcast_block_event: UnsafeCell<c::HANDLE> ,
}

#[derive(Clone, Copy)]
enum Kind {
    SRWLock,
    CriticalSection,
}

#[inline]
pub unsafe fn raw(m: &Condvar) -> *mut c::CONDITION_VARIABLE {
    debug_assert!(mem::size_of::<c::CONDITION_VARIABLE>() <= mem::size_of_val(&m.inner));
    &m.inner as *const _ as *mut _
}

impl Condvar {
    #[inline]
    pub const fn new() -> Condvar {
        Condvar {
            // This works because SRWLOCK_INIT is 0 (wrapped in a struct), so we are also properly
            // initializing an SRWLOCK here.
            inner: AtomicUsize::new(0),
        }
    }

    #[inline]
    pub unsafe fn wait(&self, mutex: &Mutex) {
        match kind() {
            Kind::SRWLock => {
                let r = c::SleepConditionVariableSRW(raw(self), mutex::raw(mutex), c::INFINITE, 0);
                debug_assert!(r != 0);
            },
            Kind::CriticalSection => {
                let inner = &*self.inner();
                /* 
                  Block access if previous broadcast hasn't finished.
                  This is just for safety and should normally not
                  affect the total time spent in this function.
                */
                c::WaitForSingleObject(*inner.broadcast_block_event.get(), c::INFINITE);

                inner.lock_waiting.lock();
                inner.waiting.set(inner.waiting.get() + 1);
                inner.lock_waiting.unlock();

                mutex.unlock();
                c::WaitForMultipleObjects(2, inner.events.as_ptr() as *const c::HANDLE, c::FALSE, c::INFINITE);
  
                inner.lock_waiting.lock();
                inner.waiting.set(inner.waiting.get() - 1);
  
                if inner.waiting.get() == 0
                {
                  /*
                    We're the last waiter to be notified or to stop waiting, so
                    reset the manual event. 
                  */
                  /* Close broadcast gate */
                  c::ResetEvent(*inner.events.get(BROADCAST).unwrap().get());
                  /* Open block gate */
                  c::SetEvent(*inner.broadcast_block_event.get());
                }
                inner.lock_waiting.unlock();
  
                mutex.lock();
            }
        }
    }

    pub unsafe fn wait_timeout(&self, mutex: &Mutex, dur: Duration) -> bool {
        match kind() {
            Kind::SRWLock => {
                let r = c::SleepConditionVariableSRW(
                    raw(self),
                    mutex::raw(mutex),
                    crate::sys::pal::dur2timeout(dur),
                    0,
                );
                if r == 0 {
                    debug_assert_eq!(os::errno() as usize, c::ERROR_TIMEOUT as usize);
                    false
                } else {
                    true
                }
            },
            Kind::CriticalSection => {
                let inner = &*self.inner();
                /* 
                  Block access if previous broadcast hasn't finished.
                  This is just for safety and should normally not
                  affect the total time spent in this function.
                */
                c::WaitForSingleObject(*inner.broadcast_block_event.get(), c::INFINITE);

                inner.lock_waiting.lock();
                inner.waiting.set(inner.waiting.get() + 1);
                inner.lock_waiting.unlock();

                mutex.unlock();
                let result= c::WaitForMultipleObjects(2, inner.events.as_ptr() as *const c::HANDLE, c::FALSE, crate::sys::pal::dur2timeout(dur));
  
                inner.lock_waiting.lock();
                inner.waiting.set(inner.waiting.get() - 1);
  
                if inner.waiting.get() == 0
                {
                  /*
                    We're the last waiter to be notified or to stop waiting, so
                    reset the manual event. 
                  */
                  /* Close broadcast gate */
                  c::ResetEvent(*inner.events.get(BROADCAST).unwrap().get());
                  /* Open block gate */
                  c::SetEvent(*inner.broadcast_block_event.get());
                }
                inner.lock_waiting.unlock();
  
                mutex.lock();

                if result == c::ERROR_TIMEOUT {
                    false
                } else {
                    true
                }
            }
        }
    }

    #[inline]
    pub fn notify_one(&self) {
        match kind() {
            Kind::SRWLock => unsafe { c::WakeConditionVariable(raw(self)) },
            Kind::CriticalSection => unsafe {
                let inner = &*self.inner();
                inner.lock_waiting.lock();
  
                if inner.waiting.get() > 0 {
                  c::SetEvent(*inner.events.get(SIGNAL).unwrap().get());
                }

                inner.lock_waiting.unlock();
            }
        }
    }

    #[inline]
    pub fn notify_all(&self) {
        match kind() {
            Kind::SRWLock => unsafe { c::WakeAllConditionVariable(raw(self)) },
            Kind::CriticalSection => unsafe {
                let inner = &*self.inner();
                inner.lock_waiting.lock();
                /*
                   The mutex protect us from broadcasting if
                   there isn't any thread waiting to open the
                   block gate after this call has closed it.
                 */
                if inner.waiting.get() > 0
                {
                  /* Close block gate */
                  c::ResetEvent(*inner.broadcast_block_event.get()); 
                  /* Open broadcast gate */
                  c::SetEvent(*inner.events.get(BROADCAST).unwrap().get());
                }

                inner.lock_waiting.unlock();
            }
        }
    }
    unsafe fn inner(&self) -> *const Inner {
        match self.inner.load(Ordering::SeqCst) {
            0 => {}
            n => return core::ptr::with_exposed_provenance(n) as *const _,
        }
        let h_event= c::CreateEventW(ptr::null_mut(),  /* no security */
                                          c::FALSE, /* auto-reset event */
                                          c::FALSE, /* non-signaled initially */
                                          ptr::null_mut()); /* unnamed */

        /* Create a manual-reset event. */
        let h_event2 = c::CreateEventW(ptr::null_mut(),  /* no security */
                                             c::TRUE,  /* manual-reset */
                                             c::FALSE, /* non-signaled initially */
                                             ptr::null_mut()); /* unnamed */


        let broadcast_block_event= c::CreateEventW(ptr::null_mut(),  /* no security */
                                                 c::TRUE,  /* manual-reset */
                                                 c::TRUE,  /* signaled initially */
                                                 ptr::null_mut()); /* unnamed */
  
        let inner = Box::new(Inner { waiting: Cell::new(0), lock_waiting: ReentrantMutex::uninitialized(), events: [UnsafeCell::new(h_event), UnsafeCell::new(h_event2)], broadcast_block_event: UnsafeCell::new(broadcast_block_event) });
        inner.lock_waiting.init();
        let inner = Box::into_raw(inner);
        match self.inner.compare_exchange(0, inner as usize, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => inner,
            Err(n) => {
                Box::from_raw(inner).lock_waiting.destroy();
                core::ptr::with_exposed_provenance(n) as *const _
            }
        }
    }
}

fn kind() -> Kind {
    if c::TryAcquireSRWLockExclusive::is_available() { Kind::SRWLock } else { Kind::CriticalSection }
}

impl Drop for Condvar {
    fn drop(&mut self) {
        match kind() {
            Kind::SRWLock => {}
            Kind::CriticalSection => match self.inner.load(Ordering::SeqCst) {
                0 => {}
                n => unsafe {
                    let inner = Box::from_raw(core::ptr::with_exposed_provenance::<Inner>(n) as *mut Inner);
                    inner.lock_waiting.destroy();
                    c::CloseHandle(*inner.events.get(SIGNAL).unwrap().get());
                    c::CloseHandle(*inner.events.get(BROADCAST).unwrap().get());
                    c::CloseHandle(*inner.broadcast_block_event.get());
                },
            },
        }
    }
}
pub struct ReentrantMutex {
    inner: MaybeUninit<UnsafeCell<c::CRITICAL_SECTION>>,
}

unsafe impl Send for ReentrantMutex {}
unsafe impl Sync for ReentrantMutex {}

impl ReentrantMutex {
    pub const fn uninitialized() -> ReentrantMutex {
        ReentrantMutex { inner: MaybeUninit::uninit() }
    }

    pub unsafe fn init(&self) {
        c::InitializeCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }

    pub unsafe fn lock(&self) {
        c::EnterCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }

    pub unsafe fn unlock(&self) {
        c::LeaveCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }

    pub unsafe fn destroy(&self) {
        c::DeleteCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }
}
