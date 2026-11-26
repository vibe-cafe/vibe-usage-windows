//! Own local CLI process trees until completion, cancellation, or app exit.
//! Browser handlers and updater installers deliberately do not use this scope.

use std::io;
use std::process::Output;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Job;

#[cfg(not(windows))]
struct Job;
#[cfg(not(windows))]
impl Job {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }
    fn empty(&self) -> bool {
        true
    }
    fn terminate(&self) {}
}

#[derive(Default)]
struct State {
    closing: bool,
    // Keep terminated jobs until their processes are actually gone. A dropped
    // timeout future must not disappear from the shutdown wait prematurely.
    jobs: Vec<Arc<Job>>,
}

#[derive(Default)]
struct Supervisor {
    state: Mutex<State>,
    attempt_finished: AtomicBool,
}

pub struct ChildGuard(Arc<Job>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.0.terminate();
    }
}

impl Supervisor {
    fn register<T>(
        &self,
        spawn: impl FnOnce(&Job) -> io::Result<T>,
    ) -> io::Result<(T, ChildGuard)> {
        let mut state = self.state.lock().unwrap();
        if state.closing {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "application is shutting down",
            ));
        }
        state.jobs.retain(|job| !job.empty());
        let job = Arc::new(Job::new()?);
        // Hold the spawn gate through creation/assignment/resume, so exit
        // cannot miss a just-created process or admit new work after closing.
        let child = spawn(&job)?;
        state.jobs.push(job.clone());
        Ok((child, ChildGuard(job)))
    }

    fn spawn(
        &self,
        command: &mut tokio::process::Command,
    ) -> io::Result<(tokio::process::Child, ChildGuard)> {
        command.kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(windows::MANAGED_FLAGS);
        self.register(|_job| {
            let mut child = command.spawn()?;
            #[cfg(windows)]
            if let Err(error) =
                _job.attach_and_resume(child.raw_handle().unwrap(), child.id().unwrap())
            {
                let _ = child.start_kill();
                return Err(error);
            }
            #[cfg(not(windows))]
            let _ = &mut child;
            Ok(child)
        })
    }

    fn output_sync(&self, command: &mut std::process::Command) -> io::Result<Output> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows::MANAGED_FLAGS);
        }
        let (child, _guard) = self.register(|_job| {
            let mut child = command.spawn()?;
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawHandle;
                if let Err(error) = _job.attach_and_resume(child.as_raw_handle(), child.id()) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
            #[cfg(not(windows))]
            let _ = &mut child;
            Ok(child)
        })?;
        child.wait_with_output()
    }

    fn begin_shutdown(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closing {
            return false;
        }
        state.closing = true;
        true
    }

    fn finish_shutdown(&self) -> bool {
        self.finish_shutdown_with_timeout(Duration::from_secs(5))
    }

    fn finish_shutdown_with_timeout(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let jobs = self.state.lock().unwrap().jobs.clone();
        for job in &jobs {
            job.terminate();
        }
        let drained = loop {
            if jobs.iter().all(|job| job.empty()) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // This flag only releases the UI exit gate after the bounded attempt;
        // it is not a claim that cleanup succeeded. On timeout return false so
        // the exit handler logs the fallback. Job handles retain kill-on-close.
        self.attempt_finished.store(true, Ordering::Release);
        drained
    }
}

fn supervisor() -> &'static Supervisor {
    static INSTANCE: OnceLock<Supervisor> = OnceLock::new();
    INSTANCE.get_or_init(Supervisor::default)
}

pub fn spawn(
    command: &mut tokio::process::Command,
) -> io::Result<(tokio::process::Child, ChildGuard)> {
    supervisor().spawn(command)
}

pub async fn output(command: &mut tokio::process::Command) -> io::Result<Output> {
    let (child, _guard) = spawn(command)?;
    child.wait_with_output().await
}

/// Like `output`, with `input` written to the child's stdin, which is then
/// closed (the CLI's `--cursor-login-stdin` reads to the end). Written while
/// the outputs drain, so neither side can block on a full pipe.
pub async fn output_with_input(command: &mut tokio::process::Command, input: &[u8]) -> io::Result<Output> {
    command.stdin(std::process::Stdio::piped());
    let (mut child, _guard) = spawn(command)?;
    let stdin = child.stdin.take();
    let write = async move {
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = stdin {
            // A child that exits without reading is not this call's error:
            // its exit status and output say what happened.
            let _ = stdin.write_all(input).await;
            let _ = stdin.shutdown().await;
        }
    };
    let (_, output) = tokio::join!(write, child.wait_with_output());
    output
}

pub fn output_sync(command: &mut std::process::Command) -> io::Result<Output> {
    supervisor().output_sync(command)
}

pub fn begin_shutdown() -> bool {
    supervisor().begin_shutdown()
}
pub fn finish_shutdown() -> bool {
    supervisor().finish_shutdown()
}
pub fn shutdown_attempt_finished() -> bool {
    supervisor().attempt_finished.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_rejects_new_processes_and_is_idempotent() {
        let scope = Supervisor::default();
        assert!(scope.begin_shutdown());
        assert!(!scope.begin_shutdown());
        let result = scope.register(|_| -> io::Result<()> {
            panic!("must not spawn after shutdown");
        });
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
        assert!(scope.finish_shutdown());
        assert!(scope.attempt_finished.load(Ordering::Acquire));
    }

    #[cfg(windows)]
    #[test]
    fn uncertain_cleanup_returns_failure_without_claiming_drained() {
        let scope = Supervisor::default();
        let job = Arc::new(Job::new().unwrap());
        job.mark_test_snapshot_uncertain();
        scope.state.lock().unwrap().jobs.push(job);
        assert!(scope.begin_shutdown());
        assert!(!scope.finish_shutdown_with_timeout(Duration::ZERO));
        assert!(scope.attempt_finished.load(Ordering::Acquire));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn shutdown_waits_for_owned_child_and_grandchild_only() {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let scope = Supervisor::default();
        // The grandchild writes its PID before sleeping. No network or accounts.
        let script = "$env:PSModulePath = $PSHOME + '\\Modules'; $p = Start-Process powershell.exe -ArgumentList '-NoProfile -NonInteractive -Command Start-Sleep -Seconds 120' -PassThru; [Console]::WriteLine($p.Id); Start-Sleep -Seconds 120";
        let mut cmd = tokio::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, guard) = scope.spawn(&mut cmd).unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let pid: u32 = tokio::time::timeout(Duration::from_secs(20), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let grandchild = windows::open_wait_handle(pid).unwrap();
        let job = scope.state.lock().unwrap().jobs[0].clone();
        let before = job.diagnostic_state(&grandchild).unwrap();
        eprintln!(
            "lifecycle before: grandchild_in_job={} tracked_handles={} pending_handles={}",
            before.0, before.1, before.2
        );
        assert!(
            before.0,
            "fixture grandchild must belong to the managed job"
        );
        // An unrelated helper must survive: never kill all node/powershell PIDs.
        let mut unrelated = tokio::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$env:PSModulePath = $PSHOME + '\\Modules'; Start-Sleep -Seconds 120",
            ])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        assert!(scope.begin_shutdown());
        assert!(scope.finish_shutdown());
        assert!(child.wait().await.is_ok());
        let after = job.diagnostic_state(&grandchild).unwrap();
        eprintln!(
            "lifecycle after: tracked_handles={} pending_handles={} grandchild_signaled={}",
            after.1,
            after.2,
            windows::signaled(&grandchild)
        );
        assert!(windows::signaled(&grandchild));
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().await.unwrap();
        drop(guard);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn dropping_operation_terminates_job_before_shutdown_completes() {
        let scope = Supervisor::default();
        let mut command = tokio::process::Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 120",
        ]);
        let (mut child, guard) = scope.spawn(&mut command).unwrap();
        drop(guard);
        tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(scope.begin_shutdown());
        assert!(scope.finish_shutdown());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn shutdown_releases_bundled_node_executable_for_uninstall() {
        use std::path::Path;
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let temporary = tempfile::tempdir().unwrap();
        let executable = temporary.path().join("node.exe");
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/node/node.exe");
        std::fs::copy(bundled, &executable).expect("fetch bundled Node before cargo test");
        let scope = Supervisor::default();
        let mut command = tokio::process::Command::new(&executable);
        command
            .args(["-e", "console.log('ready'); setInterval(() => {}, 1000)"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, _guard) = scope.spawn(&mut command).unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(20), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .as_deref(),
            Some("ready")
        );
        assert!(
            std::fs::remove_file(&executable).is_err(),
            "a running Windows image should be locked"
        );
        assert!(scope.begin_shutdown());
        assert!(scope.finish_shutdown());
        child.wait().await.unwrap();
        std::fs::remove_file(&executable)
            .expect("shutdown must release the installed runtime image");
    }
}
