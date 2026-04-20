use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::cell::UnsafeCell;
use crate::ptr;
use crate::sys::c;
use crate::sys::mutex::Mutex;
use crate::time::Duration;

const WAKEUP_MODE_NONE: usize = 0;
const WAKEUP_MODE_ONE: usize = 0x40000000;
const WAKEUP_MODE_ALL: usize = 0x80000000;
const WAKEUP_MODE_MASK: usize = WAKEUP_MODE_ONE | WAKEUP_MODE_ALL;
const SLEEPERS_COUNT_MASK: usize = !WAKEUP_MODE_MASK;

pub struct Condvar { sleepersCountAndWakeupMode: AtomicUsize,
                     sleepWakeupSemaphore: UnsafeCell<c::HANDLE>,
                     wakeOneEvent: UnsafeCell<c::HANDLE>,
                     wakeAllEvent: UnsafeCell<c::HANDLE>,
}

unsafe impl Send for Condvar {}
unsafe impl Sync for Condvar {}

impl Condvar {
    pub const fn new() -> Condvar {
        Condvar {
                  sleepersCountAndWakeupMode: AtomicUsize::new(WAKEUP_MODE_NONE),
                  sleepWakeupSemaphore: UnsafeCell::new(ptr::null_mut()),
                  wakeOneEvent: UnsafeCell::new(ptr::null_mut()),
                  wakeAllEvent: UnsafeCell::new(ptr::null_mut()),
                }
    }

    pub unsafe fn init(&mut self) {
        *self.sleepWakeupSemaphore.get() = c::CreateSemaphoreW(ptr::null_mut(), 1, 1, ptr::null_mut());
        assert!(*self.sleepWakeupSemaphore.get() != ptr::null_mut());
        *self.wakeOneEvent.get() = c::CreateEventW(ptr::null_mut(), c::FALSE, c::FALSE, ptr::null_mut());
        assert!(*self.wakeOneEvent.get() != ptr::null_mut());
        *self.wakeAllEvent.get() = c::CreateEventW(ptr::null_mut(), c::TRUE, c::FALSE, ptr::null_mut());
        assert!(*self.wakeAllEvent.get() != ptr::null_mut());
    }

    pub unsafe fn wait(&self, mutex: &Mutex) {
        Condvar::wait_timeout(self, mutex, Duration::from_secs(1000 * 365 * 86400));
    }

    pub unsafe fn wait_timeout(&self, mutex: &Mutex, dur: Duration) -> bool {
        let result = c::WaitForSingleObject(*self.sleepWakeupSemaphore.get(), c::INFINITE);
        assert!(result == c::WAIT_OBJECT_0);
        self.sleepersCountAndWakeupMode.fetch_add(1, Ordering::SeqCst);
        let mut wcwm = self.sleepersCountAndWakeupMode.load(Ordering::SeqCst);
        assert!((wcwm & WAKEUP_MODE_MASK) == 0);
        let mut success = c::ReleaseSemaphore(*self.sleepWakeupSemaphore.get(), 1, ptr::null_mut());
        assert!(success != 0);
        mutex.unlock();
        let handeles = [*self.wakeOneEvent.get(), *self.wakeAllEvent.get()];
        let waitResult = c::WaitForMultipleObjects(2, handeles.as_ptr(), c::FALSE, super::dur2timeout(dur));

        let sub: i32;
        if waitResult == c::WAIT_OBJECT_0 {
           sub = 1 | WAKEUP_MODE_ONE as i32;
         } else {
           sub = 1;
         }
        
        wcwm = self.sleepersCountAndWakeupMode.fetch_add(-sub as usize, Ordering::SeqCst) - sub as usize;

        let wakeupMode = wcwm & WAKEUP_MODE_MASK;
        let sleepersCount = wcwm & SLEEPERS_COUNT_MASK;

        let mut releaseSleepWakeupSemaphore = false;

        if waitResult == c::WAIT_OBJECT_0 {
            releaseSleepWakeupSemaphore = true;
        } else if waitResult == c::WAIT_TIMEOUT && wakeupMode == WAKEUP_MODE_ONE && sleepersCount == 0 {
            success = c::ResetEvent(*self.wakeOneEvent.get());
            assert!(success != 0);
            self.sleepersCountAndWakeupMode.store(WAKEUP_MODE_NONE, Ordering::SeqCst);
            releaseSleepWakeupSemaphore = true;
        } else if wakeupMode == WAKEUP_MODE_ALL && sleepersCount == 0 {
            success = c::ResetEvent(*self.wakeAllEvent.get());
            assert!(success != 0);
            self.sleepersCountAndWakeupMode.store(WAKEUP_MODE_NONE, Ordering::SeqCst);
            releaseSleepWakeupSemaphore = true;
        } else if waitResult == c::WAIT_TIMEOUT && super::dur2timeout(dur) != c::INFINITE ||
                  (waitResult == c::WAIT_OBJECT_0 + 1 &&wakeupMode == WAKEUP_MODE_ALL) {
        } else {
            panic!("invalid wakeup condition");
        }

        if releaseSleepWakeupSemaphore {
            success = c::ReleaseSemaphore(*self.sleepWakeupSemaphore.get(), 1, ptr::null_mut());
            assert!(success != 0);
        }

        mutex.lock();

        if waitResult == c::WAIT_TIMEOUT {
           c::SetLastError(c::ERROR_TIMEOUT);
           return false;
        }

        true 
    }

    pub unsafe fn notify_one(&self) {
         Condvar::wakeup(self, WAKEUP_MODE_ONE, *self.wakeOneEvent.get());
    }

    pub unsafe fn notify_all(&self) {
         Condvar::wakeup(self, WAKEUP_MODE_ALL, *self.wakeAllEvent.get());
    }

    pub unsafe fn destroy(&self) {
         assert!(self.sleepersCountAndWakeupMode.load(Ordering::SeqCst) == 0);
         let mut r = c::CloseHandle(*self.sleepWakeupSemaphore.get());
         assert!(r != 0);
         r = c::CloseHandle(*self.wakeOneEvent.get());
         assert!(r != 0);
         r = c::CloseHandle(*self.wakeAllEvent.get());
         assert!(r != 0);
    }

    unsafe fn wakeup(&self, wakeupMode: usize, wakeEvent: c::HANDLE) {
        let result = c::WaitForSingleObject(*self.sleepWakeupSemaphore.get(), c::INFINITE);
        assert!(result == c::WAIT_OBJECT_0);
        let wcwm = self.sleepersCountAndWakeupMode.fetch_add(wakeupMode, Ordering::SeqCst);
        let sleepersCount = wcwm & SLEEPERS_COUNT_MASK;
        if sleepersCount > 0 {
            let success = c::SetEvent(wakeEvent);
            assert!(success != 0);
        } else {
            self.sleepersCountAndWakeupMode.store(WAKEUP_MODE_NONE, Ordering::SeqCst);
            let success = c::ReleaseSemaphore(*self.sleepWakeupSemaphore.get(), 1, ptr::null_mut());
            assert!(success != 0);
        }

    }
}
