//! A small local https server for the managed source tests: canned answers per path, and a log
//! of what each request carried.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// A test certificate authority and a `localhost` certificate it signed.
pub(crate) struct TestCa {
    ca_pem: String,
    leaf: CertificateDer<'static>,
    leaf_key: Vec<u8>,
}

impl TestCa {
    /// A new authority, trusted by https clients built on this thread.
    pub(crate) fn trusted() -> Self {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let leaf_key = rcgen::KeyPair::generate().unwrap();
        let leaf = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .unwrap()
            .signed_by(&leaf_key, &ca, &ca_key)
            .unwrap();
        let test_ca = Self {
            ca_pem: ca.pem(),
            leaf: leaf.der().clone(),
            leaf_key: leaf_key.serialize_der(),
        };
        super::remote::trust_test_root(
            reqwest::Certificate::from_pem(test_ca.ca_pem.as_bytes()).unwrap(),
        );
        test_ca
    }
}

/// What the server answers on one path.
#[derive(Debug, Clone)]
pub(crate) enum Route {
    Body(Vec<u8>),
    Status(u16),
    Redirect(String),
}

/// One request the server received.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub body: Vec<u8>,
}

/// A running server on `https://localhost:<port>`.
pub(crate) struct TestServer {
    port: u16,
    routes: Arc<Mutex<HashMap<String, Route>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl TestServer {
    pub(crate) fn start(ca: &TestCa) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![ca.leaf.clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(ca.leaf_key.clone())),
            )
            .unwrap();
        let config = Arc::new(config);
        let routes = Arc::new(Mutex::new(HashMap::new()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (thread_routes, thread_seen) = (routes.clone(), seen.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (config, routes, seen) =
                    (config.clone(), thread_routes.clone(), thread_seen.clone());
                std::thread::spawn(move || {
                    let connection = rustls::ServerConnection::new(config).unwrap();
                    let stream = rustls::StreamOwned::new(connection, stream);
                    let _ = serve(stream, &routes, &seen);
                });
            }
        });
        Self { port, routes, seen }
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("https://localhost:{}{path}", self.port)
    }

    pub(crate) fn route(&self, path: &str, route: Route) {
        self.routes.lock().unwrap().insert(path.to_string(), route);
    }

    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub(crate) fn seen_at(&self, path: &str) -> Vec<Seen> {
        self.seen()
            .into_iter()
            .filter(|seen| seen.path == path)
            .collect()
    }
}

fn serve(
    stream: impl Read + Write,
    routes: &Mutex<HashMap<String, Route>>,
    seen: &Mutex<Vec<Seen>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut authorization = None;
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "authorization" => authorization = Some(value.trim().to_string()),
                "content-length" => length = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    seen.lock().unwrap().push(Seen {
        method,
        path: path.clone(),
        authorization,
        body,
    });
    let route = routes.lock().unwrap().get(&path).cloned();
    let (status, extra, body) = match route {
        Some(Route::Body(body)) => (200, String::new(), body),
        Some(Route::Status(status)) => (status, String::new(), Vec::new()),
        Some(Route::Redirect(to)) => (302, format!("Location: {to}\r\n"), Vec::new()),
        None => (404, String::new(), Vec::new()),
    };
    let stream = reader.get_mut();
    write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}
