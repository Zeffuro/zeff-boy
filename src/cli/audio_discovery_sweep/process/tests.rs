use super::*;
use std::io::{Read, Write};

fn arguments(role: &str) -> Vec<OsString> {
    [
        "--ignored".to_owned(),
        "--exact".to_owned(),
        format!("{}::{role}", module_path!().split_once("::").unwrap().1),
        "--nocapture".to_owned(),
        "--test-threads=1".to_owned(),
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

fn fixture(role: &str, timeout: Duration, cancel: &AtomicBool) -> Result<ProcessOutcome> {
    run(
        &std::env::current_exe()?,
        &arguments(role),
        &std::env::current_dir()?,
        timeout,
        cancel,
    )
}

fn captured_pids(outcome: &ProcessOutcome) -> Vec<u32> {
    String::from_utf8_lossy(&outcome.stdout)
        .split_whitespace()
        .filter_map(|word| word.strip_prefix("child_pid=")?.parse().ok())
        .collect()
}

fn assert_reaped(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!running(pid), "child {pid} remains running");
}

#[cfg(windows)]
fn running(pid: u32) -> bool {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // SAFETY: OpenProcess accepts a numeric PID and returns an owned handle on success.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        return false;
    }
    // SAFETY: the handle is valid and owned; the zero-time wait cannot block.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) == WAIT_TIMEOUT }
}

#[cfg(unix)]
fn running(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    if std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| rest.starts_with(" Z"))
    }) {
        return false;
    }
    // SAFETY: signal zero only inspects the identified process.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[test]
fn normal_exit_preserves_status_stdin_eof_and_bounded_log_tails() -> Result<()> {
    let result = fixture(
        "child_normal",
        Duration::from_secs(10),
        &AtomicBool::new(false),
    )?;
    assert_eq!(result.termination, Termination::Exited);
    assert_eq!(result.exit_code, Some(23));
    assert_eq!(result.stdout.len(), RETAIN_BYTES);
    assert!(result.stdout_bytes > RETAIN_BYTES as u64);
    assert!(result.stdout.ends_with(b"stdin_eof tail-marker\n"));
    assert_eq!(result.stderr, b"stderr-marker\n");
    assert_eq!(result.stderr_bytes, result.stderr.len() as u64);
    Ok(())
}

#[test]
fn timeout_kills_and_reaps_the_actual_child() -> Result<()> {
    let result = fixture(
        "child_hang",
        Duration::from_millis(800),
        &AtomicBool::new(false),
    )?;
    assert_eq!(result.termination, Termination::TimedOut);
    assert!(result.elapsed_ms < 5000);
    let pids = captured_pids(&result);
    assert_eq!(pids.len(), 1);
    assert_reaped(pids[0]);
    Ok(())
}

#[test]
fn each_output_stream_has_an_independent_hard_byte_limit() -> Result<()> {
    for (role, stdout) in [
        ("child_stdout_overflow", true),
        ("child_stderr_overflow", false),
    ] {
        let result = fixture(role, Duration::from_secs(10), &AtomicBool::new(false))?;
        assert_eq!(result.termination, Termination::OutputLimit);
        let (retained, count) = if stdout {
            (&result.stdout, result.stdout_bytes)
        } else {
            (&result.stderr, result.stderr_bytes)
        };
        assert_eq!(retained.len(), RETAIN_BYTES);
        assert!(retained.iter().all(|&value| value == b'Z'));
        assert!((OUTPUT_LIMIT + 1..=OUTPUT_LIMIT + 8192).contains(&count));
        assert!(result.elapsed_ms < 5000);
    }
    Ok(())
}

#[test]
fn cancellation_before_spawn_and_while_running_is_bounded() -> Result<()> {
    let result = run(
        Path::new("missing-sweep-child"),
        &[],
        Path::new("."),
        Duration::from_secs(10),
        &AtomicBool::new(true),
    )?;
    assert_eq!(result.termination, Termination::Cancelled);
    assert_eq!(result.exit_code, None);
    assert_eq!(result.stdout_bytes, 0);
    let cancel = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(800));
            cancel.store(true, Ordering::Relaxed);
        });
        fixture("child_hang", Duration::from_secs(10), &cancel)
    })?;
    assert_eq!(result.termination, Termination::Cancelled);
    assert!(result.elapsed_ms < 5000);
    let pids = captured_pids(&result);
    assert_eq!(pids.len(), 1);
    assert_reaped(pids[0]);
    Ok(())
}

#[test]
fn inherited_descendant_pipes_do_not_delay_parent_exit_cleanup() -> Result<()> {
    let result = fixture(
        "child_descendant",
        Duration::from_secs(10),
        &AtomicBool::new(false),
    )?;
    assert_eq!(result.termination, Termination::Exited);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.elapsed_ms < 5000);
    let pids = captured_pids(&result);
    assert_eq!(pids.len(), 2);
    for pid in pids {
        assert_reaped(pid);
    }
    Ok(())
}

#[test]
fn panic_cleanup_reaps_a_spawned_child() -> Result<()> {
    let guard = ChildGuard::spawn(
        &std::env::current_exe()?,
        &arguments("child_hang"),
        &std::env::current_dir()?,
    )?;
    let pid = guard.child.id();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _guard = guard;
        panic!("exercise child cleanup during unwinding");
    }));
    assert!(result.is_err());
    assert_reaped(pid);
    Ok(())
}

#[test]
fn invalid_spawn_and_timeout_fail_before_execution() {
    for timeout in [Duration::ZERO, Duration::from_secs(1)] {
        assert!(
            run(
                Path::new("missing-sweep-child"),
                &[],
                Path::new("."),
                timeout,
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
}

fn print_pid() {
    println!("child_pid={}", std::process::id());
    std::io::stdout().flush().unwrap();
}

#[test]
#[ignore = "child process fixture"]
fn child_normal() {
    let mut input = Vec::new();
    assert_eq!(std::io::stdin().read_to_end(&mut input).unwrap(), 0);
    std::io::stdout().write_all(&vec![b'A'; 96 * 1024]).unwrap();
    println!("stdin_eof tail-marker");
    eprintln!("stderr-marker");
    std::io::stdout().flush().unwrap();
    std::io::stderr().flush().unwrap();
    std::process::exit(23);
}

#[test]
#[ignore = "child process fixture"]
fn child_hang() {
    print_pid();
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
#[ignore = "child process fixture"]
fn child_stdout_overflow() {
    let mut output = std::io::stdout().lock();
    for _ in 0..256 {
        output.write_all(&[b'Z'; 8192]).unwrap();
    }
    output.flush().unwrap();
}

#[test]
#[ignore = "child process fixture"]
fn child_stderr_overflow() {
    let mut output = std::io::stderr().lock();
    for _ in 0..256 {
        output.write_all(&[b'Z'; 8192]).unwrap();
    }
    output.flush().unwrap();
}

#[test]
#[ignore = "child process fixture"]
#[allow(
    clippy::zombie_processes,
    reason = "the runner owns descendant cleanup"
)]
fn child_descendant() {
    print_pid();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(arguments("child_hang")).stdin(Stdio::null());
    #[cfg(windows)]
    native::configure(&mut command);
    let child = command.spawn().unwrap();
    let _child = child;
    std::thread::sleep(Duration::from_millis(400));
}
