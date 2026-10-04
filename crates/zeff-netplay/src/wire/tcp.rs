use super::*;

pub struct Connection {
    pub(super) io: Driver,
    pub(super) stream: TcpStream,
    pub(super) player: Player,
    pub(super) secret: [u8; 32],
    pub(super) transcript: [u8; 32],
    pub(super) send_sequence: u64,
    pub(super) receive_sequence: u64,
    pub(super) sent_close: bool,
    pub(super) received_close: bool,
    pub(super) terminal: bool,
    pub(super) cancellation: Option<Arc<AtomicBool>>,
}

pub fn admit(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
) -> Result<Connection> {
    admit_inner(
        stream,
        player,
        identity,
        secret,
        None,
        ConnectionScope::Loopback,
    )
}

pub fn admit_cancellable(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    cancellation: Arc<AtomicBool>,
) -> Result<Connection> {
    admit_inner(
        stream,
        player,
        identity,
        secret,
        Some(cancellation),
        ConnectionScope::Loopback,
    )
}

pub fn admit_scoped(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    scope: ConnectionScope,
) -> Result<Connection> {
    admit_inner(stream, player, identity, secret, None, scope)
}

pub fn admit_cancellable_scoped(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    scope: ConnectionScope,
    cancellation: Arc<AtomicBool>,
) -> Result<Connection> {
    admit_inner(stream, player, identity, secret, Some(cancellation), scope)
}

fn admit_inner(
    mut stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    cancellation: Option<Arc<AtomicBool>>,
    scope: ConnectionScope,
) -> Result<Connection> {
    let result = handshake(
        &mut stream,
        player,
        identity,
        secret,
        cancellation.as_deref(),
        scope,
    );
    match result {
        Ok((io, transcript)) => Ok(Connection {
            io,
            stream,
            player,
            secret: *secret,
            transcript,
            send_sequence: 0,
            receive_sequence: 0,
            sent_close: false,
            received_close: false,
            terminal: false,
            cancellation,
        }),
        Err(error) => {
            let _ = stream.shutdown(Shutdown::Both);
            Err(error)
        }
    }
}

fn handshake(
    stream: &mut TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    cancellation: Option<&AtomicBool>,
    scope: ConnectionScope,
) -> Result<(Driver, [u8; 32])> {
    let deadline = Instant::now() + IO_BUDGET;
    check_deadline(deadline, cancellation)?;
    stream.set_nonblocking(false)?;
    scope.validate_connection(stream)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IO_BUDGET))?;
    stream.set_write_timeout(Some(IO_BUDGET))?;
    let mut driver = Driver::new(stream)?;
    let local = hello(player, identity)?;
    driver.write(stream, &local, deadline, cancellation)?;
    let mut remote = [0; HELLO_LEN];
    driver
        .read(stream, &mut remote, deadline, cancellation)
        .context("reading admission hello")?;
    ensure!(&remote[..4] == MAGIC, "invalid admission magic");
    ensure!(
        remote[4..6] == VERSION.to_be_bytes(),
        "unsupported wire version"
    );
    ensure!(remote[6] == role(other(player)), "incompatible player role");
    ensure!(
        remote[71..BUILD_INFO_OFFSET] == local[71..BUILD_INFO_OFFSET],
        "session identity mismatch"
    );
    let remote_info = BuildInfo::decode(&remote[BUILD_INFO_OFFSET..])?;
    build::admit_builds(
        &identity.build,
        &identity.build_info,
        &remote[39..71].try_into()?,
        &remote_info,
    )?;
    let (host, client) = if player == Player::One {
        (&local, &remote)
    } else {
        (&remote, &local)
    };
    let transcript: [u8; 32] = Sha256::new()
        .chain_update(AUTH_DOMAIN)
        .chain_update(host)
        .chain_update(client)
        .finalize()
        .into();
    let local_tag = tag(secret, &[AUTH_DOMAIN, host, client, &[role(player)]]);
    driver.write(stream, &local_tag, deadline, cancellation)?;
    let mut remote_tag = [0; TAG_LEN];
    driver
        .read(stream, &mut remote_tag, deadline, cancellation)
        .context("reading authentication")?;
    verify(
        secret,
        &[AUTH_DOMAIN, host, client, &[role(other(player))]],
        &remote_tag,
    )?;
    let ready = tag(secret, &[READY_DOMAIN, &transcript, &[role(player)]]);
    driver.write(stream, &ready, deadline, cancellation)?;
    driver
        .read(stream, &mut remote_tag, deadline, cancellation)
        .context("reading Ready barrier")?;
    verify(
        secret,
        &[READY_DOMAIN, &transcript, &[role(other(player))]],
        &remote_tag,
    )?;
    check_deadline(deadline, cancellation)?;
    Ok((driver, transcript))
}
