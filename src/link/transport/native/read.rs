use std::io;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

const WAIT_MS: i32 = 100;

#[cfg(test)]
#[derive(Default)]
pub(super) struct ReaderProgress {
    pub(super) timeouts: std::sync::atomic::AtomicUsize,
    pub(super) bytes: std::sync::atomic::AtomicUsize,
}

pub(super) fn read_exact(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    connected: &AtomicBool,
    #[cfg(test)] progress: &ReaderProgress,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        if !connected.load(Ordering::Acquire) {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let ready = match wait_readable(stream) {
            Ok(ready) => ready,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if !connected.load(Ordering::Acquire) {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if !ready {
            #[cfg(test)]
            progress.timeouts.fetch_add(1, Ordering::Release);
            continue;
        }
        match receive(stream, &mut buffer[offset..]) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(bytes) => {
                offset += bytes;
                #[cfg(test)]
                progress.bytes.fetch_add(bytes, Ordering::Release);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn receive(stream: &mut TcpStream, buffer: &mut [u8]) -> io::Result<usize> {
    use std::io::Read;

    // Winsock select guarantees this single read for the sole reader will not block.
    stream.read(buffer)
}

#[cfg(unix)]
fn receive(stream: &mut TcpStream, buffer: &mut [u8]) -> io::Result<usize> {
    use std::os::fd::AsRawFd;

    // Per-call nonblocking receive handles spurious readiness without changing the writer.
    let bytes = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            libc::MSG_DONTWAIT,
        )
    };
    if bytes < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(bytes as usize)
    }
}

#[cfg(windows)]
fn wait_readable(stream: &TcpStream) -> io::Result<bool> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        FD_SET, SOCKET_ERROR, TIMEVAL, WSAEINTR, WSAGetLastError, select,
    };

    let mut readable = FD_SET {
        fd_count: 1,
        ..FD_SET::default()
    };
    readable.fd_array[0] = stream.as_raw_socket() as _;
    let timeout = TIMEVAL {
        tv_sec: 0,
        tv_usec: WAIT_MS * 1_000,
    };
    // The stream stays owned and the initialized set/timeout remain valid for select.
    let result = unsafe {
        select(
            0,
            &mut readable,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &timeout,
        )
    };
    if result == SOCKET_ERROR {
        // Read the calling thread's Winsock error before another Winsock call.
        let error = unsafe { WSAGetLastError() };
        return Err(if error == WSAEINTR {
            io::Error::from(io::ErrorKind::Interrupted)
        } else {
            io::Error::from_raw_os_error(error)
        });
    }
    Ok(result > 0)
}

#[cfg(unix)]
fn wait_readable(stream: &TcpStream) -> io::Result<bool> {
    use std::os::fd::AsRawFd;

    let mut readable = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // The stream owns this descriptor and poll receives one valid pollfd.
    let result = unsafe { libc::poll(&mut readable, 1, WAIT_MS) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if readable.revents & libc::POLLNVAL != 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    Ok(result > 0 && readable.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
}

#[cfg(all(test, unix))]
#[test]
fn receive_without_data_returns_would_block_and_preserves_blocking_writer() {
    use std::net::TcpListener;
    use std::os::fd::AsRawFd;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    assert_eq!(
        receive(&mut stream, &mut [0; 1]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    // The live descriptor supports F_GETFL; the per-call flag must leave O_NONBLOCK clear.
    let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(flags & libc::O_NONBLOCK, 0);
}
