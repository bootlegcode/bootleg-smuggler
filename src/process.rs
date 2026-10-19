//! Unix process adapter: process-group signalling, orphan adoption and waitpid reaping.
use std::{
    ffi::OsString,
    io,
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Command, ExitStatus},
    time::Duration,
};
use tokio::{
    signal::unix::{Signal, SignalKind, signal},
    time::{Instant, sleep},
};

pub fn exec_through(command: &[OsString]) -> io::Error {
    Command::new(&command[0]).args(&command[1..]).exec()
}
pub struct Supervisor {
    pid: i32,
    term: Signal,
    interrupt: Signal,
    hup: Signal,
    quit: Signal,
}
impl Supervisor {
    pub fn launch(command: &[OsString]) -> io::Result<Self> {
        // Install handlers before launch so PID 1 never loses a termination request.
        let term = signal(SignalKind::terminate())?;
        let interrupt = signal(SignalKind::interrupt())?;
        let hup = signal(SignalKind::hangup())?;
        let quit = signal(SignalKind::quit())?;
        #[cfg(target_os = "linux")]
        // SAFETY: prctl with a scalar argument, no pointers. Adopt grandchildren even outside PID 1 tests.
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let child = Command::new(&command[0])
            .args(&command[1..])
            .process_group(0)
            .spawn()?;
        Ok(Self {
            pid: child.id() as i32,
            term,
            interrupt,
            hup,
            quit,
        })
    }
    fn forward(&self, signal: i32) {
        // SAFETY: negative pid addresses only the child's dedicated process group.
        unsafe {
            libc::kill(-self.pid, signal);
        }
    }
    fn reap(&self) -> io::Result<Option<ExitStatus>> {
        let mut main = None;
        loop {
            let mut status = 0;
            // SAFETY: valid status pointer; nonblocking wait on our own children.
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            if pid == 0 {
                return Ok(main);
            }
            if pid < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ECHILD) {
                    return Ok(main);
                }
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if pid == self.pid {
                main = Some(ExitStatus::from_raw(status));
            }
        }
    }
    pub async fn wait(mut self) -> io::Result<i32> {
        let mut deadline = None;
        loop {
            if let Some(status) = self.reap()? {
                self.forward(libc::SIGTERM);
                sleep(Duration::from_millis(100)).await;
                self.forward(libc::SIGKILL);
                // Give adopted children a bounded opportunity to exit and be reaped.
                for _ in 0..10 {
                    self.reap()?;
                    sleep(Duration::from_millis(10)).await;
                }
                return Ok(status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                self.forward(libc::SIGKILL);
                deadline = None;
            }
            let received = tokio::select! {
                _ = self.term.recv() => Some(libc::SIGTERM),
                _ = self.interrupt.recv() => Some(libc::SIGINT),
                _ = self.hup.recv() => Some(libc::SIGHUP),
                _ = self.quit.recv() => Some(libc::SIGQUIT),
                _ = sleep(Duration::from_millis(20)) => None,
            };
            if let Some(received) = received {
                self.forward(received);
                if received != libc::SIGHUP && deadline.is_none() {
                    deadline = Some(Instant::now() + Duration::from_secs(10));
                }
            }
        }
    }
}
