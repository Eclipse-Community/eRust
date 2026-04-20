use crate::cell::UnsafeCell;
use crate::sys::c;
use crate::sys::mutex::Mutex;
use crate::ptr;
use crate::time::Duration;

const WAKEUP_MODE_NONE: i32 = 0;
const WAKEUP_MODE_ONE: u32 = 0x40000000;
const WAKEUP_MODE_ALL: u32 = 0x80000000;
const WAKEUP_MODE_MASK: u32 = WAKEUP_MODE_ONE | WAKEUP_MODE_ALL;
const SLEEPERS_COUNT_MASK: u32 = !WAKEUP_MODE_MASK;

pub struct Condvar { sleepersCountAndWakeupMode_: UnsafeCell<i32>,
                     sleepWakeupSemaphore_: UnsafeCell<c::HANDLE>,
                     wakeOneEvent_: UnsafeCell<c::HANDLE>,
                     wakeAllEvent_: UnsafeCell<c::HANDLE>,
}

unsafe impl Send for Condvar {}
unsafe impl Sync for Condvar {}

impl Condvar {
    pub const fn new() -> Condvar {
        Condvar {
                  sleepersCountAndWakeupMode_: UnsafeCell::new(WAKEUP_MODE_NONE),
                  sleepWakeupSemaphore_: UnsafeCell::new(ptr::null_mut()),
                  wakeOneEvent_: UnsafeCell::new(ptr::null_mut()),
                  wakeAllEvent_: UnsafeCell::new(ptr::null_mut()),
                }
    }

    pub unsafe fn init(&mut self) {
        *self.sleepWakeupSemaphore_.get() = c::CreateSemaphoreW(ptr::null_mut(), 1, 1, ptr::null_mut());
        assert!(*self.sleepWakeupSemaphore_.get() != ptr::null_mut());
        *self.wakeOneEvent_.get() = c::CreateEventW(ptr::null_mut(), c::FALSE, c::FALSE, ptr::null_mut());
        assert!(*self.wakeOneEvent_.get() != ptr::null_mut());
        *self.wakeAllEvent_.get() = c::CreateEventW(ptr::null_mut(), c::TRUE, c::FALSE, ptr::null_mut());
        assert!(*self.wakeAllEvent_.get() != ptr::null_mut());
    }

    pub unsafe fn wait(&self, mutex: &Mutex) {
        Condvar::wait_timeout(self, mutex, Duration::from_secs(1000 * 365 * 86400));
    }

    pub unsafe fn wait_timeout(&self, mutex: &Mutex, dur: Duration) -> bool {
        let result = c::WaitForSingleObject(*self.sleepWakeupSemaphore_.get(), c::INFINITE);
        assert!(result == c::WAIT_OBJECT_0);
        let mut wcwm: u32 = c::InterlockedIncrement(self.sleepersCountAndWakeupMode_.get()) as u32;
        assert!((wcwm & WAKEUP_MODE_MASK) == 0);
        let mut success = c::ReleaseSemaphore(*self.sleepWakeupSemaphore_.get(), 1, ptr::null_mut());
        assert!(success != 0);
        mutex.unlock();
        let handeles = [*self.wakeOneEvent_.get(), *self.wakeAllEvent_.get()];
        let waitResult = c::WaitForMultipleObjects(2, handeles.as_ptr(), c::FALSE, super::dur2timeout(dur));

        let sub: i32;
        if waitResult == c::WAIT_OBJECT_0 {
           sub = 1 | WAKEUP_MODE_ONE as i32;
        } else {
           sub = 1;
        }
        
        wcwm = (c::InterlockedExchangeAdd(self.sleepersCountAndWakeupMode_.get(), -sub) - sub) as u32;

        let wakeupMode = wcwm & WAKEUP_MODE_MASK;
        let sleepersCount = wcwm & SLEEPERS_COUNT_MASK;

        let mut releaseSleepWakeupSemaphore = false;

        if waitResult == c::WAIT_OBJECT_0 {
            releaseSleepWakeupSemaphore = true;
        } else if waitResult == c::WAIT_TIMEOUT && wakeupMode == WAKEUP_MODE_ONE && sleepersCount == 0 {
            success = c::ResetEvent(*self.wakeOneEvent_.get());
            assert!(success != 0);
            *self.sleepersCountAndWakeupMode_.get() = WAKEUP_MODE_NONE;
            releaseSleepWakeupSemaphore = true;
        } else if wakeupMode == WAKEUP_MODE_ALL && sleepersCount == 0 {
            success = c::ResetEvent(*self.wakeAllEvent_.get());
            assert!(success != 0);
            *self.sleepersCountAndWakeupMode_.get() = WAKEUP_MODE_NONE;
            releaseSleepWakeupSemaphore = true;
        } else if waitResult == c::WAIT_TIMEOUT && super::dur2timeout(dur) != c::INFINITE ||
                  (waitResult == c::WAIT_OBJECT_0 + 1 &&wakeupMode == WAKEUP_MODE_ALL) {
        } else {
            panic!("invalid wakeup condition");
        }

        if releaseSleepWakeupSemaphore {
            success = c::ReleaseSemaphore(*self.sleepWakeupSemaphore_.get(), 1, ptr::null_mut());
            assert!(success != 0);
        }

        mutex.lock();

        if waitResult == c::WAIT_TIMEOUT {
           c::SetLastError(c::ERROR_TIMEOUT);
           return false;
        }

        true 
    }

    #[inline]
    pub unsafe fn notify_one(&self) {
         Condvar::wakeup(self, WAKEUP_MODE_ONE, *self.wakeOneEvent_.get());
    }

    #[inline]
    pub unsafe fn notify_all(&self) {
         Condvar::wakeup(self, WAKEUP_MODE_ALL, *self.wakeAllEvent_.get());
    }

    pub unsafe fn destroy(&self) {
         assert!(*self.sleepersCountAndWakeupMode_.get() == 0);
         let mut r = c::CloseHandle(*self.sleepWakeupSemaphore_.get());
         assert!(r != 0);
         r = c::CloseHandle(*self.wakeOneEvent_.get());
         assert!(r != 0);
         r = c::CloseHandle(*self.wakeAllEvent_.get());
         assert!(r != 0);
    }

    unsafe fn wakeup(&self, wakeupMode: u32, wakeEvent: c::HANDLE) {
        let result = c::WaitForSingleObject(*self.sleepWakeupSemaphore_.get(), c::INFINITE);
        assert!(result == c::WAIT_OBJECT_0);
        let wcwm: u32 = c::InterlockedExchangeAdd(self.sleepersCountAndWakeupMode_.get(),
                                             wakeupMode as i32) as u32;
        let sleepersCount = wcwm & SLEEPERS_COUNT_MASK;
        if sleepersCount > 0 {
            let success = c::SetEvent(wakeEvent);
            assert!(success != 0);
        } else {
            *self.sleepersCountAndWakeupMode_.get() = WAKEUP_MODE_NONE;
            let success = c::ReleaseSemaphore(*self.sleepWakeupSemaphore_.get(), 1, ptr::null_mut());
            assert!(success != 0);
        }

    }

}
