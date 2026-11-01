use serde_json::Value;
use std::{fs, path::PathBuf};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) struct TestDir(pub(crate) PathBuf);
impl TestDir {
    pub(crate) fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-publish-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    pub(crate) fn zip(&self) -> PathBuf {
        let path = self.0.join("AuthorAddon.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file(
            "AuthorAddon/AuthorAddon.toc",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"## Interface: 120000\n## Title: AuthorAddon\n")
            .unwrap();
        zip.finish().unwrap();
        path
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("all publishing file handles must be released");
    }
}

pub(crate) struct Request {
    pub(crate) line: String,
    pub(crate) headers: String,
    pub(crate) body: Vec<u8>,
}
pub(crate) enum Reply {
    Json(Value),
    Bytes(Vec<u8>),
    Drop,
    Raw(&'static str),
    Headers(Vec<(String, String)>),
}
pub(crate) struct Server {
    pub(crate) url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    pub(crate) fn new(replies: Vec<Reply>) -> Self {
        Self::with_hook(replies, |_| {})
    }
    pub(crate) fn with_hook(
        replies: Vec<Reply>,
        mut hook: impl FnMut(usize) + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            for (index, reply) in replies.into_iter().enumerate() {
                let mut connection = loop {
                    match listener.accept() {
                        Ok((connection, _)) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "expected native HTTP request was not sent"
                            );
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                let request = read_request(&mut connection);
                captured.lock().unwrap().push(request);
                hook(index);
                match reply {
                    Reply::Bytes(bytes) => {
                        write!(connection,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/zip\r\nConnection: close\r\n\r\n",bytes.len()).unwrap();
                        connection.write_all(&bytes).unwrap();
                    }
                    Reply::Drop => {}
                    Reply::Raw(response) => connection.write_all(response.as_bytes()).unwrap(),
                    Reply::Headers(headers) => {
                        write!(
                            connection,
                            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n"
                        )
                        .unwrap();
                        for (name, value) in headers {
                            write!(connection, "{name}: {value}\r\n").unwrap();
                        }
                        connection.write_all(b"\r\n").unwrap();
                    }
                    Reply::Json(value) => {
                        let bytes = serde_json::to_vec(&value).unwrap();
                        write!(connection,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",bytes.len()).unwrap();
                        connection.write_all(&bytes).unwrap();
                    }
                }
            }
        });
        Self {
            url,
            requests,
            worker: Some(worker),
        }
    }
    pub(crate) fn finish(mut self) -> Vec<Request> {
        self.worker.take().unwrap().join().unwrap();
        Arc::try_unwrap(self.requests)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap()
    }
}
fn read_request(connection: &mut TcpStream) -> Request {
    connection.set_nonblocking(false).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let n = connection.read(&mut chunk).unwrap();
        assert_ne!(n, 0);
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
        assert!(bytes.len() < 65536);
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find_map(|(key, value)| {
            key.eq_ignore_ascii_case("content-length")
                .then_some(value.trim())
        })
        .map(|length| length.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(length < 16 * 1024 * 1024);
    while bytes.len() < header_end + length {
        let mut chunk = [0u8; 4096];
        let n = connection.read(&mut chunk).unwrap();
        assert_ne!(n, 0);
        bytes.extend_from_slice(&chunk[..n]);
    }
    Request {
        line: String::from_utf8(
            bytes[..bytes.windows(2).position(|part| part == b"\r\n").unwrap()].to_vec(),
        )
        .unwrap(),
        headers,
        body: bytes[header_end..].to_vec(),
    }
}
