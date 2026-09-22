use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

pub(super) trait Pipe: Read + AsRawFd {}
impl<T: Read + AsRawFd> Pipe for T {}

pub(super) fn configure(command: &mut Command) {
    command.process_group(0);
}

pub(super) fn prepare_pipe(pipe: &impl Pipe) -> io::Result<()> {
    // SAFETY: both fcntl calls use a live pipe descriptor and integer flags.
    let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn read_available(pipe: &mut impl Pipe, buffer: &mut [u8]) -> io::Result<usize> {
    match pipe.read(buffer) {
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(0)
        }
        result => result,
    }
}

pub(super) struct Group(Option<libc::pid_t>);

impl Group {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self(None))
    }

    pub(super) fn attach(&mut self, child: &Child) -> io::Result<()> {
        self.0 = Some(child.id().try_into().map_err(io::Error::other)?);
        Ok(())
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        let Some(pid) = self.0 else { return Ok(()) };
        // SAFETY: the negative PID selects only the child's newly created process group.
        if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }
}
