use crate::sys::c;

pub fn fill_bytes(mut bytes: &mut [u8]) {
    while !bytes.is_empty() {
        let len = bytes.len().try_into().unwrap_or(u32::MAX);
        let ret = unsafe { c::RtlGenRandom(bytes.as_mut_ptr().cast(), len) };
        assert_ne!(ret, 0, "failed to generate random data");
        bytes = &mut bytes[len as usize..];
    }
}
