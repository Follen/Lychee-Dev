use super::*;
use std::{
    io::{BufRead, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) fn run(args: &[String]) -> Result<()> {
    if args.len() != 5 {
        return Err(fault(
            "Private worker requires its exact supervisor handshake",
        ));
    }
    let token = &args[1];
    if token.len() != 32
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
        || std::env::var(START_ENV).ok().as_deref() != Some(token.as_str())
    {
        return Err(fault("Private worker startup identity mismatch"));
    }
    let owner = lycheedev_windows::ProcessIdentity {
        pid: args[3]
            .parse()
            .map_err(|_| fault("Invalid supervisor PID"))?,
        created: args[4]
            .parse()
            .map_err(|_| fault("Invalid supervisor creation identity"))?,
    };
    let own = lycheedev_windows::process::Process::open(std::process::id()).map_err(fault)?;
    let authenticated = (|| {
        let parent = lycheedev_windows::process::Process::open(owner.pid).map_err(fault)?;
        let result = (|| {
            if own.parent_pid().map_err(fault)? != owner.pid
                || parent.identity() != &owner
                || parent.image_path().map_err(fault)?.canonicalize()?
                    != std::env::current_exe()?.canonicalize()?
            {
                return Err(fault(
                    "Private Data worker requires its actual same-binary supervisor",
                ));
            }
            parent.verify().map_err(fault)
        })();
        lycheedev_core::metadata::owned_write::finish_owned(result, parent.close().map_err(fault))
    })();
    lycheedev_core::metadata::owned_write::finish_owned(authenticated, own.close().map_err(fault))?;
    let port: u16 = args[2]
        .parse()
        .map_err(|_| fault("Invalid private receipt endpoint"))?;
    // An explicitly invoked private entry still cannot bypass the OS bound.
    // Handshake bytes are separately bounded before engine construction.
    let input = std::io::stdin();
    let mut input = input.lock();
    let mut bytes = Vec::new();
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return Err(fault("Missing private startup frame"));
        }
        let end = buffer.iter().position(|b| *b == b'\n');
        let count = end.map_or(buffer.len(), |n| n + 1);
        if bytes.len() + count > 65536 {
            return Err(fault("Private startup frame too large"));
        }
        bytes.extend_from_slice(&buffer[..count]);
        input.consume(count);
        if end.is_some() {
            break;
        }
    }
    // Drop stdin lock without constructing a second buffered reader: Stdin's
    // shared buffer preserves any bytes read ahead for the production ingress.
    drop(input);
    let startup: Startup = serde_json::from_slice(&bytes)?;
    if startup.lifetime_ms == 0 || startup.lifetime_ms > 3_600_000 {
        return Err(fault("Invalid private worker lifetime"));
    }
    let context = lycheedev_data::request::DataContext {
        home: startup.home,
        cwd: startup.cwd,
        source: startup.source,
        control: lycheedev_core::control::OperationControl::uncancelable(
            Instant::now() + Duration::from_millis(startup.lifetime_ms),
        ),
    };
    let job = lycheedev_windows::BrokerJob::own_current_process(
        startup.session.retained_bytes + (256 << 20),
    )
    .map_err(fault)?;
    let result = (|| {
        let stream = TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
            Duration::from_secs(5),
        )?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.set_nodelay(true)?;
        let stream = Arc::new(Mutex::new(stream));
        let token = token.clone();
        let observer = Arc::new(move |event: ReceiptEvent| -> Result<()> {
            let (event, id) = match event {
                ReceiptEvent::Ready => ("ready", None),
                ReceiptEvent::Completed(id) => ("completed", Some(id)),
            };
            let receipt = Receipt {
                protocol: RECEIPT_PROTOCOL.into(),
                token: token.clone(),
                pid: std::process::id(),
                event: event.into(),
                id,
            };
            // Send each small receipt as one frame. Direct serialization to
            // TCP splits it into tiny writes and can delay ready/completion.
            let mut bytes = serde_json::to_vec(&receipt)?;
            bytes.push(b'\n');
            let mut stream = stream
                .lock()
                .map_err(|_| fault("Private receipt lock poisoned"))?;
            stream.write_all(&bytes)?;
            stream.flush()?;
            Ok(())
        });
        super::transport::serve_stdio_observed(context, startup.session, observer)
    })();
    lycheedev_core::metadata::owned_write::finish_owned(result, job.close().map_err(fault))
}
