use std::os::windows::io::RawHandle;
use winapi::ctypes::c_void;
use winapi::um::synchapi::ReleaseMutex;

pub struct LockHandle {
    pub(crate) fd: RawHandle,
}

impl Drop for LockHandle {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.fd as *mut c_void);
        }
    }
}
