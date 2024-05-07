use std::os::windows::io::RawHandle;
use winapi::ctypes::c_void;
use winapi::um::synchapi::ReleaseMutex;

pub struct LockHandle {
    pub(crate) handle: RawHandle,
}

impl Drop for LockHandle {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.handle as *mut c_void);
        }
    }
}
