use crate::marker::PhantomData;
use crate::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle};
use crate::slice;
use crate::sys::{c};

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct IoSlice<'a> {
    vec: c::WSABUF,
    _p: PhantomData<&'a [u8]>,
}

impl<'a> IoSlice<'a> {
    #[inline]
    pub fn new(buf: &'a [u8]) -> IoSlice<'a> {
        assert!(buf.len() <= c::ULONG::MAX as usize);
        IoSlice {
            vec: c::WSABUF {
                len: buf.len() as c::ULONG,
                buf: buf.as_ptr() as *mut u8 as *mut c::CHAR,
            },
            _p: PhantomData,
        }
    }

    #[inline]
    pub fn advance(&mut self, n: usize) {
        if (self.vec.len as usize) < n {
            panic!("advancing IoSlice beyond its length");
        }

        unsafe {
            self.vec.len -= n as c::ULONG;
            self.vec.buf = self.vec.buf.add(n);
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.vec.buf as *mut u8, self.vec.len as usize) }
    }
}

#[repr(transparent)]
pub struct IoSliceMut<'a> {
    vec: c::WSABUF,
    _p: PhantomData<&'a mut [u8]>,
}

impl<'a> IoSliceMut<'a> {
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> IoSliceMut<'a> {
        assert!(buf.len() <= c::ULONG::MAX as usize);
        IoSliceMut {
            vec: c::WSABUF { len: buf.len() as c::ULONG, buf: buf.as_mut_ptr() as *mut c::CHAR },
            _p: PhantomData,
        }
    }

    #[inline]
    pub fn advance(&mut self, n: usize) {
        if (self.vec.len as usize) < n {
            panic!("advancing IoSliceMut beyond its length");
        }

        unsafe {
            self.vec.len -= n as c::ULONG;
            self.vec.buf = self.vec.buf.add(n);
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.vec.buf as *mut u8, self.vec.len as usize) }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.vec.buf as *mut u8, self.vec.len as usize) }
    }
}

pub fn is_terminal(h: &impl AsHandle) -> bool {
    unsafe { handle_is_console(h.as_handle()) }
}

unsafe fn handle_is_console(handle: BorrowedHandle<'_>) -> bool {
    let handle = handle.as_raw_handle();

    // A null handle means the process has no console.
    if handle.is_null() {
        return false;
    }

    let mut out = 0;
    if c::GetConsoleMode(handle, &mut out) != 0 {
        // False positives aren't possible. If we got a console then we definitely have a console.
        return true;
    }

    // At this point, we *could* have a false negative. We can determine that this is a true
    // negative if we can detect the presence of a console on any of the standard I/O streams. If
    // another stream has a console, then we know we're in a Windows console and can therefore
    // trust the negative.
    for std_handle in [c::STD_INPUT_HANDLE, c::STD_OUTPUT_HANDLE, c::STD_ERROR_HANDLE] {
        let std_handle = c::GetStdHandle(std_handle);
        if !std_handle.is_null()
            && std_handle != handle
            && c::GetConsoleMode(std_handle, &mut out) != 0
        {
            return false;
        }
    }
    return false;
}
