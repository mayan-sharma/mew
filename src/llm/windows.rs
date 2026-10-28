use std::ffi::OsStr;
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// llama-server in a job that Windows kills when Mew's handle closes, even if Mew crashes.
pub struct Process {
    child: Child,
    job: HANDLE,
}

// The job handle is only used from whichever thread holds the mutex.
unsafe impl Send for Process {}

impl Process {
    pub fn spawn(exe: &Path, args: &[&OsStr], log: File) -> Result<Self> {
        let child = Command::new(exe)
            .args(args)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        let job = kill_with_parent(&child)?;
        Ok(Self { child, job })
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        unsafe { CloseHandle(self.job) };
    }
}

fn kill_with_parent(child: &Child) -> Result<HANDLE> {
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            bail!("CreateJobObject failed");
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
        Ok(job)
    }
}
