use std::ffi::OsStr;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use anyhow::Result;

/// Runs llama-server ("$@"), ends it once Mew is gone, even after a crash, and exits with the server's status.
const GUARD: &str = r#""$@" & server=$!
while kill -0 "$PPID" 2>/dev/null && kill -0 "$server" 2>/dev/null; do sleep 1; done
kill "$server" 2>/dev/null
wait "$server""#;

/// llama-server under a watching shell, in a process group of their own.
pub struct Process(Child);

impl Process {
    pub fn spawn(exe: &Path, args: &[&OsStr], log: File) -> Result<Self> {
        let child = Command::new("/bin/sh")
            .args(["-c", GUARD, "sh"])
            .arg(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .process_group(0) // Ctrl+C on a dev build stops Mew; the shell then ends the server
            .spawn()?;
        Ok(Self(child))
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.0.try_wait()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        unsafe { libc::kill(-(self.0.id() as libc::pid_t), libc::SIGTERM) };
        let _ = self.0.wait();
    }
}
