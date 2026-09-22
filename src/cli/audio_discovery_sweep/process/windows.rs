use std::io::{self, Read};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::PeekNamedPipe;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

pub(super) trait Pipe: Read + AsRawHandle {}
impl<T: Read + AsRawHandle> Pipe for T {}

pub(super) fn configure(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

pub(super) fn prepare_pipe(_pipe: &impl Pipe) -> io::Result<()> {
    Ok(())
}

pub(super) fn read_available(pipe: &mut impl Pipe, buffer: &mut [u8]) -> io::Result<usize> {
    let mut available = 0;
    // SAFETY: the pipe is live and only the available-byte output pointer is supplied.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            &mut available,
            null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if matches!(
            error.raw_os_error().map(|value| value as u32),
            Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
        ) {
            return Ok(0);
        }
        return Err(error);
    }
    let count = buffer.len().min(available as usize);
    if count == 0 {
        return Ok(0);
    }
    pipe.read(&mut buffer[..count])
}

pub(super) struct Group(OwnedHandle);

impl Group {
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: an unnamed job has no borrowed security attributes or name.
        let raw = unsafe { CreateJobObjectW(null(), null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned an owned, valid handle.
        let group = Self(unsafe { OwnedHandle::from_raw_handle(raw) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the handle and initialized structure remain live for this call.
        if unsafe {
            SetInformationJobObject(
                group.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(group)
    }

    pub(super) fn attach(&mut self, child: &Child) -> io::Result<()> {
        // SAFETY: both handles are live; attachment occurs immediately after spawning.
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        // SAFETY: this job is owned exclusively by this runner.
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
