//! The job object calls, the only unsafe code in the workspace
#![allow(unsafe_code)]

use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::process::Command;
use std::ptr;

use windows_sys::Win32::Foundation::{FALSE, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::{Reach, Signal};

// What a process ended with the job gets, as `Child::kill` gives its own
const ENDED: u32 = 1;

// `GetExitCodeProcess` answers this for a process that has not ended
const STILL_ACTIVE: u32 = 259;

pub(super) fn lead(command: &mut Command) -> &mut Command {
    command
}

pub(super) fn lead_tokio(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    command
}

// A job whose last handle closing ends every process in it. None for a group
// another image started, which Windows never hands over.
#[derive(Debug)]
pub(super) struct Tree(Option<OwnedHandle>);

impl Tree {
    pub(super) fn of(child: &std::process::Child) -> io::Result<Self> {
        Self::holding(child.as_raw_handle())
    }

    pub(super) fn of_tokio(child: &tokio::process::Child) -> io::Result<Self> {
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("the child has been waited on"))?;
        Self::holding(process)
    }

    // Made and filled in one step, so a process never sits in a job that does
    // not yet end it.
    fn holding(process: RawHandle) -> io::Result<Self> {
        let job = create()?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `job` is a live job handle, and `limits` is the structure the
        // extended-limit class reads, of the size passed.
        let set = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                size_of_u32::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>(),
            )
        };
        check(set)?;
        // SAFETY: both handles are live. The process handle is borrowed from a
        // child the caller holds, and the call does not keep it.
        check(unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) })?;
        Ok(Self(Some(job)))
    }

    pub(super) fn from_group(_pgid: u32) -> Self {
        Self(None)
    }

    pub(super) fn alone(self) -> Self {
        self
    }

    // Windows has no request to end that a process may refuse, so everything but
    // an interrupt ends the job at once. A console hands Ctrl-C to its
    // processes itself, and a job has no way to.
    pub(super) fn send(&self, signal: Signal) -> Reach {
        let Some(job) = &self.0 else {
            return Reach::Whole;
        };
        if signal == Signal::Interrupt {
            return Reach::Whole;
        }
        // SAFETY: the handle is a live job handle.
        match check(unsafe { TerminateJobObject(job.as_raw_handle(), ENDED) }) {
            Ok(()) => Reach::Whole,
            Err(_) => Reach::Partial,
        }
    }

    pub(super) fn running(&self) -> bool {
        let Some(job) = &self.0 else {
            return false;
        };
        let mut counts = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the handle is a live job handle, and `counts` is the structure
        // the basic-accounting class writes, of the size passed.
        let queried = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                ptr::from_mut(&mut counts).cast(),
                size_of_u32::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>(),
                ptr::null_mut(),
            )
        };
        check(queried).is_ok() && counts.ActiveProcesses > 0
    }
}

pub(super) fn alive(pid: u32) -> bool {
    // SAFETY: a plain call with no pointers.
    let process: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) };
    if process.is_null() {
        return false;
    }
    // SAFETY: `process` is a handle this call opened and nothing else owns.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut code = 0;
    // SAFETY: the handle is live, and `code` is where the call writes.
    let read = unsafe { GetExitCodeProcess(process.as_raw_handle(), &raw mut code) };
    check(read).is_ok_and(|()| code == STILL_ACTIVE)
}

fn create() -> io::Result<OwnedHandle> {
    // SAFETY: a null security descriptor and name are allowed: the job is
    // unnamed and its handle is not inherited.
    let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `job` is a handle the call just made and nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(job) })
}

// A BOOL result as the error Windows holds for the thread when it is zero
fn check(result: i32) -> io::Result<()> {
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn size_of_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).expect("a Windows structure is far smaller than 4 GiB")
}
