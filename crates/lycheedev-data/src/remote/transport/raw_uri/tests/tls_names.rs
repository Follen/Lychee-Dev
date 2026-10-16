//! Actual private-root TLS loopback. This does not install a root, bypass TLS
//! validation, or claim to exercise Go's default Windows system-root verifier.
use super::*;
use std::sync::{Arc, Mutex};

const ROOT: &[u8] = include_bytes!("../tls_fixtures/root.pem");
const IDENTITY: &[u8] = include_bytes!("../tls_fixtures/server.p12");

#[derive(Debug)]
struct ObserveHello {
    socket: TcpStream,
    input: Arc<Mutex<Vec<u8>>>,
}
impl std::io::Read for ObserveHello {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let n = std::io::Read::read(&mut self.socket, output)?;
        let mut input = self.input.lock().unwrap();
        let retained = n.min(65540 - input.len());
        input.extend_from_slice(&output[..retained]);
        Ok(n)
    }
}
impl std::io::Write for ObserveHello {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        std::io::Write::write(&mut self.socket, input)
    }
    fn flush(&mut self) -> io::Result<()> {
        std::io::Write::flush(&mut self.socket)
    }
}

#[test]
fn private_root_tls_observes_all_dot_names_and_strict_certificate_results() {
    let mut observations = Vec::new();
    // The fixed Go private-RootCAs verifier accepts one DNS trailing dot, but
    // not a second. Its ClientHello SNI removes every trailing dot. Wrong-name
    // rejection is essential: matching SNI alone cannot prove strict checking.
    for (name, accepted, sni) in [
        ("example.test", true, "example.test"),
        ("example.test.", true, "example.test"),
        ("example.test..", false, "example.test"),
        ("example.test...", false, "example.test"),
        ("wrong.test", false, "wrong.test"),
    ] {
        let identity = native_tls::Identity::from_pkcs12(IDENTITY, "public-test-fixture").unwrap();
        let acceptor = native_tls::TlsAcceptor::new(identity).unwrap();
        let input = Arc::new(Mutex::new(Vec::new()));
        let recorded = input.clone();
        let (address, peer) = server(move |socket| {
            let input = ObserveHello {
                socket,
                input: recorded,
            };
            match acceptor.accept(input) {
                Ok(mut stream) => {
                    std::io::Write::write_all(&mut stream, b"ok").map_err(|e| e.to_string())
                }
                Err(error) => Err(error.to_string()),
            }
        });
        let (control, budget) = scope();
        let result: Result<()> = run(async {
            let mut builder = native_tls::TlsConnector::builder();
            builder.disable_built_in_roots(true).add_root_certificate(
                native_tls::Certificate::from_pem(ROOT).map_err(|e| http(e.to_string()))?,
            );
            let (verification, connector) = platform_tls_with_builder(name, builder)?;
            let operation = async {
                let socket = tokio::net::TcpStream::connect(&address)
                    .await
                    .map_err(|e| {
                        http(e.to_string()).with_details(serde_json::json!({"phase":"connect"}))
                    })?;
                let mut stream = connector
                    .connect(&verification, socket)
                    .await
                    .map_err(|e| {
                        http(e.to_string()).with_details(serde_json::json!({"phase":"tls"}))
                    })?;
                let mut reply = [0; 2];
                tokio::io::AsyncReadExt::read_exact(&mut stream, &mut reply)
                    .await
                    .map_err(|e| {
                        http(e.to_string()).with_details(serde_json::json!({"phase":"reply"}))
                    })?;
                if reply != *b"ok" {
                    return Err(http("unexpected private TLS fixture reply"));
                }
                Ok(())
            };
            controlled(operation, &control, Some(&budget), control.deadline()).await?
        });
        // Always join before comparing results. Even a TLS failure owns its
        // accepted socket; server() bounds accept/read/write at five seconds.
        let server_result = peer.join().unwrap();
        let bytes = input.lock().unwrap().clone();
        let observed_sni = if bytes.is_empty() {
            None
        } else {
            client_hello_sni(&mut io::Cursor::new(bytes))
                .map(|bytes| String::from_utf8(bytes).unwrap())
        };
        let record = serde_json::json!({
            "input": name,
            "verificationInput": tls_name(name),
            "sni": observed_sni,
            "accepted": result.is_ok(),
            "error": result.err(),
            "stop": control.check().err(),
            "server": server_result,
            "expectedPrivateGoAccepted": accepted,
            "expectedGoSni": sni,
            "trust": "private test root only",
            "os": std::env::consts::OS,
        });
        println!("LYCHEE_TLS_NAME_OBSERVATION {}", record);
        observations.push(record);
    }
    // Collect every outcome before reporting a mismatch, preserving all finite
    // cases even if the platform cannot represent Go's separate SNI/verify names.
    let differences: Vec<_> = observations
        .iter()
        .filter(|v| {
            v["accepted"] != v["expectedPrivateGoAccepted"]
                || v["sni"] != v["expectedGoSni"]
                || !v["stop"].is_null()
                || (v["accepted"] == false && v["error"]["details"]["phase"] != "tls")
        })
        .collect();
    assert!(
        differences.is_empty(),
        "private-root TLS name differential: {}",
        serde_json::to_string(&observations).unwrap()
    );
}
