use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{
    DuplicateHandle, DUPLICATE_SAME_ACCESS, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenThread, ResumeThread, WaitForSingleObject,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    THREAD_SUSPEND_RESUME,
};

pub const MANAGED_FLAGS: u32 = CREATE_NO_WINDOW | CREATE_SUSPENDED;

#[derive(Default)]
struct ExitWait {
    prepared: bool,
    uncertain: bool,
    // Keep kernel handles, not PIDs: a terminating descendant can leave job
    // accounting before its process object becomes signaled; PIDs can be reused.
    processes: Vec<OwnedHandle>,
}

pub struct Job {
    handle: OwnedHandle,
    exit_wait: Mutex<ExitWait>,
}
impl Job {
    pub fn new() -> io::Result<Self> {
        // Unnamed, non-inheritable job: its lifetime belongs only to this app.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self {
            handle: unsafe { OwnedHandle::from_raw_handle(raw) },
            exit_wait: Mutex::new(ExitWait::default()),
        };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.handle(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn handle(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }

    pub fn attach_and_resume(&self, process: RawHandle, pid: u32) -> io::Result<()> {
        // CREATE_SUSPENDED prevents even a fast launcher from spawning an
        // unowned grandchild before assignment. Rust 1.88 does not expose the
        // primary thread handle on stable, so find only this owned process's
        // initial thread through the documented ToolHelp API.
        if unsafe { AssignProcessToJobObject(self.handle(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut duplicate = std::ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                process,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        self.exit_wait
            .lock()
            .unwrap()
            .processes
            .push(unsafe { OwnedHandle::from_raw_handle(duplicate) });
        let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut entry: THREADENTRY32 = unsafe { zeroed() };
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let raw = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if raw.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
                if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "suspended child thread not found",
        ))
    }

    fn close_admission(&self) -> io::Result<()> {
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        if unsafe {
            QueryInformationJobObject(
                self.handle(),
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as *mut _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // At least one live parent is needed to spawn. Limiting this private
        // job to one active process closes descendant admission before taking
        // the snapshot; existing members are not killed by lowering the limit.
        limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        if unsafe {
            SetInformationJobObject(
                self.handle(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn member_handles(&self) -> io::Result<Vec<OwnedHandle>> {
        // usize storage supplies the native alignment needed by the flexible
        // ProcessIdList. Bound growth/fail closed rather than truncate a tree.
        let offset = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
        let mut capacity = 16usize;
        loop {
            let bytes = offset + capacity * size_of::<usize>();
            let mut buffer = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let raw = buffer
                .as_mut_ptr()
                .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            let ok = unsafe {
                QueryInformationJobObject(
                    self.handle(),
                    JobObjectBasicProcessIdList,
                    raw.cast(),
                    bytes as u32,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) && capacity < 65536 {
                    capacity *= 2;
                    continue;
                }
                return Err(error);
            }
            let count = unsafe { (*raw).NumberOfProcessIdsInList as usize };
            if count > capacity || unsafe { (*raw).NumberOfAssignedProcesses as usize } > count {
                return Err(io::Error::other("incomplete job process snapshot"));
            }
            let ids = unsafe {
                std::slice::from_raw_parts(
                    buffer.as_ptr().cast::<u8>().add(offset).cast::<usize>(),
                    count,
                )
            };
            let mut handles = Vec::with_capacity(count);
            for &pid in ids {
                let raw = unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        pid as u32,
                    )
                };
                if raw.is_null() {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                        // The process object is already gone, not just exiting.
                        continue;
                    }
                    return Err(error);
                }
                let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
                if self.contains(&handle)? {
                    handles.push(handle);
                }
                // If the PID was reused outside this job, never wait on/kill it.
            }
            return Ok(handles);
        }
    }

    fn contains(&self, process: &OwnedHandle) -> io::Result<bool> {
        let mut member = 0;
        if unsafe { IsProcessInJob(process.as_raw_handle(), self.handle(), &mut member) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(member != 0)
    }

    #[cfg(test)]
    pub fn diagnostic_state(&self, process: &OwnedHandle) -> io::Result<(bool, usize, usize)> {
        let member = self.contains(process)?;
        let wait = self.exit_wait.lock().unwrap();
        Ok((
            member,
            wait.processes.len(),
            wait.processes.iter().filter(|p| !signaled(p)).count(),
        ))
    }

    #[cfg(test)]
    pub fn mark_test_snapshot_uncertain(&self) {
        self.exit_wait.lock().unwrap().uncertain = true;
    }

    pub fn terminate(&self) {
        let mut wait = self.exit_wait.lock().unwrap();
        if !wait.prepared {
            match self.close_admission().and_then(|()| self.member_handles()) {
                Ok(handles) => wait.processes.extend(handles),
                Err(error) => {
                    wait.uncertain = true;
                    log::warn!(
                        "Cannot confirm CLI process snapshot; Windows error {:?}",
                        error.raw_os_error()
                    );
                }
            }
            wait.prepared = true;
        }
        // Cancellation and shutdown use the same retained snapshot. Never
        // replace it with an empty post-termination accounting snapshot.
        if unsafe { TerminateJobObject(self.handle(), 1) } == 0 {
            wait.uncertain = true;
            log::warn!(
                "Cannot terminate CLI job; Windows error {:?}",
                io::Error::last_os_error().raw_os_error()
            );
        }
    }

    pub fn empty(&self) -> bool {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle(),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        let wait = self.exit_wait.lock().unwrap();
        ok != 0
            && info.ActiveProcesses == 0
            && !wait.uncertain
            && wait.processes.iter().all(signaled)
    }
}

#[cfg(test)]
pub fn open_wait_handle(pid: u32) -> io::Result<OwnedHandle> {
    let raw = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

pub fn signaled(handle: &OwnedHandle) -> bool {
    unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) == WAIT_OBJECT_0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};

    #[test]
    fn zero_accounting_does_not_override_an_unsignaled_handle() {
        let job = Job::new().unwrap();
        // Deterministically simulate the observed kernel teardown gap: job
        // accounting is zero but a retained waitable handle is not signaled.
        let raw = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        assert!(!raw.is_null());
        job.exit_wait
            .lock()
            .unwrap()
            .processes
            .push(unsafe { OwnedHandle::from_raw_handle(raw) });
        assert!(
            !job.empty(),
            "zero accounting alone must not complete cleanup"
        );
        job.terminate();
        job.terminate();
        assert!(
            !job.empty(),
            "cancellation/shutdown must retain the pending handle"
        );
        assert_ne!(unsafe { SetEvent(raw) }, 0);
        assert!(job.empty());
    }

    #[test]
    fn an_uncertain_snapshot_is_never_reported_as_drained() {
        let job = Job::new().unwrap();
        job.exit_wait.lock().unwrap().uncertain = true;
        assert!(!job.empty());
    }

    #[tokio::test]
    async fn closed_admission_preserves_existing_process_and_rejects_new_members() {
        use std::process::Stdio;
        use std::time::Duration;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command;
        use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
        use windows_sys::Win32::System::JobObjects::{
            JobObjectAssociateCompletionPortInformation, JOBOBJECT_ASSOCIATE_COMPLETION_PORT,
        };
        use windows_sys::Win32::System::SystemServices::JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT;
        use windows_sys::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus};
        let node = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/node/node.exe");
        let job = Job::new().unwrap();
        // Associate while the private Job is empty, before any child can emit
        // notifications. The owned port and matching key isolate the evidence.
        let raw_port =
            unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1) };
        assert!(!raw_port.is_null(), "{}", io::Error::last_os_error());
        let port = unsafe { OwnedHandle::from_raw_handle(raw_port) };
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: job.handle(),
            CompletionPort: port.as_raw_handle(),
        };
        assert_ne!(
            unsafe {
                SetInformationJobObject(
                    job.handle(),
                    JobObjectAssociateCompletionPortInformation,
                    &association as *const _ as *const _,
                    size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
                )
            },
            0,
            "{}",
            io::Error::last_os_error()
        );
        let take_notification = || {
            let mut message = 0;
            let mut key = 0;
            let mut overlapped = std::ptr::null_mut();
            let ok = unsafe {
                GetQueuedCompletionStatus(
                    port.as_raw_handle(),
                    &mut message,
                    &mut key,
                    &mut overlapped,
                    0,
                )
            };
            if ok == 0 {
                assert_eq!(
                    io::Error::last_os_error().raw_os_error(),
                    Some(WAIT_TIMEOUT as i32)
                );
                None
            } else {
                assert_eq!(
                    key,
                    job.handle() as usize,
                    "notification from a different Job"
                );
                Some(message)
            }
        };
        // First prove the identical child works before applying the limit.
        // A failed spawn alone cannot prove Job rejection (ENOENT, ETIMEDOUT,
        // antivirus, etc.). Below we require this private Job's active-process
        // limit notification. No inner spawn timeout.
        let script = r#"
            const spawn = () => require('node:child_process').spawnSync(
                process.execPath, ['-e', "console.log('child-ran')"], {encoding:'utf8'});
            const before = spawn();
            console.log(!before.error && before.status === 0 && before.stdout.trim() === 'child-ran'
                ? 'ready' : 'baseline-failed');
            process.stdin.once('data', () => {
                const after = spawn();
                console.log(after.error && after.error.code !== 'ETIMEDOUT' && after.status === null
                    && !after.stdout?.includes('child-ran') ? 'rejected' : 'admitted');
            });
            setInterval(() => {}, 1000);
        "#;
        let mut parent = Command::new(&node)
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .creation_flags(MANAGED_FLAGS)
            .spawn()
            .unwrap();
        job.attach_and_resume(parent.raw_handle().unwrap(), parent.id().unwrap())
            .unwrap();
        let mut lines = BufReader::new(parent.stdout.take().unwrap()).lines();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .as_deref(),
            Some("ready")
        );
        while let Some(message) = take_notification() {
            assert_ne!(message, JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT);
        }
        job.close_admission().unwrap();
        assert!(parent.try_wait().unwrap().is_none());
        parent
            .stdin
            .take()
            .unwrap()
            .write_all(b"spawn\n")
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .as_deref(),
            Some("rejected")
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if take_notification() == Some(JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("spawn error must correspond to this Job's active-process limit notification");
        assert!(parent.try_wait().unwrap().is_none());
        job.terminate();
        tokio::time::timeout(Duration::from_secs(10), parent.wait())
            .await
            .unwrap()
            .unwrap();
        // The process handle can signal before Job accounting finishes.
        tokio::time::timeout(Duration::from_secs(10), async {
            while !job.empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("terminated Job must drain within the shutdown budget");
    }
}
