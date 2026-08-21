#[cfg(unix)]
pub fn pause_proc(pid: i32) {
    // SAFETY: kill with SIGSTOP only addresses the verified child pid retained by Session. It
    // neither dereferences pointers nor transfers ownership.
    unsafe {
        libc::kill(pid, libc::SIGSTOP);
    }
}

#[cfg(unix)]
pub fn cont_proc(pid: i32) {
    // SAFETY: kill with SIGCONT only addresses the verified child pid retained by Session. It
    // neither dereferences pointers nor transfers ownership.
    unsafe {
        libc::kill(pid, libc::SIGCONT);
    }
}

#[cfg(windows)]
mod windows {
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{
        OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
    };

    struct OwnedHandle(HANDLE);

    impl OwnedHandle {
        fn new(handle: HANDLE) -> Option<Self> {
            (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(Self(handle))
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: this wrapper is the unique owner of a non-null, non-invalid Win32 handle.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    fn for_each_process_thread(pid: u32, mut operation: impl FnMut(HANDLE)) {
        // SAFETY: the snapshot call does not borrow Rust memory. The returned handle is wrapped
        // immediately so all return paths close it.
        let Some(snapshot) =
            OwnedHandle::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })
        else {
            return;
        };
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            cntUsage: 0,
            th32ThreadID: 0,
            th32OwnerProcessID: 0,
            tpBasePri: 0,
            tpDeltaPri: 0,
            dwFlags: 0,
        };
        // SAFETY: entry has the required size and remains valid for the complete enumeration.
        let mut has_entry = unsafe { Thread32First(snapshot.0, &mut entry) } != 0;
        while has_entry {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: OpenThread receives a thread id returned by the active snapshot.
                if let Some(thread) = OwnedHandle::new(unsafe {
                    OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID)
                }) {
                    operation(thread.0);
                }
            }
            // SAFETY: snapshot and entry remain live and valid until enumeration completes.
            has_entry = unsafe { Thread32Next(snapshot.0, &mut entry) } != 0;
        }
    }

    pub fn pause_proc(pid: i32) {
        for_each_process_thread(pid as u32, |thread| {
            // SAFETY: the handle was opened with THREAD_SUSPEND_RESUME for this operation.
            unsafe {
                SuspendThread(thread);
            }
        });
    }

    pub fn cont_proc(pid: i32) {
        for_each_process_thread(pid as u32, |thread| {
            // SAFETY: the handle was opened with THREAD_SUSPEND_RESUME for this operation.
            unsafe {
                ResumeThread(thread);
            }
        });
    }
}

#[cfg(windows)]
pub use windows::{cont_proc, pause_proc};
