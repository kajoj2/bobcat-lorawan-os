// SPDX-License-Identifier: MIT
//! A small HTTP/1.1 server for the panel (`/`), its data (`/api/status`), the text report (`/api/status.txt`) and
//! the eMMC page's requests (`/api/storage...`), plus the HTTP client side used for ChirpStack's gRPC-web API and by
//! `lorawan-diag`.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MAX_CONNECTIONS: usize = 16;
/// Request line, headers and body together; the panel only ever sends small forms.
const MAX_REQUEST: u64 = 16384;

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Writes a response body of unknown length (a backup), after the head; the connection closes at its end.
pub type Streamer = Box<dyn FnOnce(&mut dyn Write) -> io::Result<()> + Send>;

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub headers: Vec<(&'static str, String)>,
    pub stream: Option<Streamer>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Response {
        Response {
            status,
            content_type,
            body: body.into(),
            headers: Vec::new(),
            stream: None,
        }
    }

    pub fn streamed(content_type: &'static str, stream: Streamer) -> Response {
        Response {
            stream: Some(stream),
            ..Response::new(200, content_type, Vec::new())
        }
    }

    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Response {
        self.headers.push((name, value.into()));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn write_response(stream: &mut TcpStream, r: Response, head_only: bool) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\n",
        r.status,
        reason(r.status),
        r.content_type
    );
    if r.stream.is_none() {
        head.push_str(&format!("Content-Length: {}\r\n", r.body.len()));
    }
    for (k, v) in &r.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(
        "Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
    );
    stream.write_all(head.as_bytes())?;
    if head_only {
        return stream.flush();
    }
    match r.stream {
        Some(f) => {
            // A backup can pause while the compressor reads past empty space; give the client time, not 5 s.
            stream.set_write_timeout(Some(Duration::from_secs(300)))?;
            f(stream)?;
        }
        None => stream.write_all(&r.body)?,
    }
    stream.flush()
}

fn read_request(stream: &TcpStream) -> io::Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_REQUEST);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let mut req = Request {
        method: method.to_string(),
        path: target.split('?').next().unwrap_or("/").to_string(),
        headers,
        body: Vec::new(),
    };
    if let Some(len) = req
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok())
    {
        if len > MAX_REQUEST {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request too large",
            ));
        }
        req.body.resize(len as usize, 0);
        reader.read_exact(&mut req.body)?;
    }
    Ok(req)
}

pub type Route = dyn Fn(&Request) -> Response + Send + Sync;

fn handle(mut stream: TcpStream, route: &Route) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let req = match read_request(&stream) {
        Ok(r) => r,
        Err(_) => {
            return write_response(
                &mut stream,
                Response::new(400, "text/plain", "bad request\n"),
                false,
            )
        }
    };
    let r = match req.method.as_str() {
        "GET" | "HEAD" | "POST" => route(&req),
        _ => Response::new(405, "text/plain", "method not allowed\n"),
    };
    write_response(&mut stream, r, req.method == "HEAD")
}

/// Serves forever; one thread per connection, at most MAX_CONNECTIONS at once.
pub fn serve(listener: TcpListener, route: Arc<Route>) -> ! {
    let active = Arc::new(AtomicUsize::new(0));
    loop {
        let Ok((mut stream, _)) = listener.accept() else {
            continue;
        };
        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            let _ = write_response(
                &mut stream,
                Response::new(503, "text/plain", "busy\n"),
                false,
            );
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (active, route) = (active.clone(), route.clone());
        thread::spawn(move || {
            let _ = handle(stream, &*route);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

pub fn bind(addr: &str) -> io::Result<TcpListener> {
    // "[::]:80" also takes IPv4 on Linux (bindv6only = 0); fall back to IPv4 only where IPv6 is off.
    TcpListener::bind(addr).or_else(|e| match addr.strip_prefix("[::]") {
        Some(port) => TcpListener::bind(format!("0.0.0.0{port}")),
        None => Err(e),
    })
}

// --- client ---------------------------------------------------------------------------------------------------

pub struct ClientResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ClientResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_body(reader: &mut impl BufRead, headers: &[(String, String)]) -> io::Result<Vec<u8>> {
    let get = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.as_str())
    };
    let mut body = Vec::new();
    if get("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size)?;
            let size = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
            if size == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + size, 0);
            reader.read_exact(&mut body[start..])?;
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf)?;
        }
    } else if let Some(len) = get("content-length").and_then(|v| v.trim().parse::<usize>().ok()) {
        body.resize(len, 0);
        reader.read_exact(&mut body)?;
    } else {
        reader.read_to_end(&mut body)?;
    }
    Ok(body)
}

/// One request over a fresh connection. `host` is "addr:port".
pub fn request(
    host: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> io::Result<ClientResponse> {
    let mut stream = TcpStream::connect(host)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad status line"))?;
    let mut hdrs = Vec::new();
    loop {
        let mut l = String::new();
        if reader.read_line(&mut l)? == 0 || l == "\r\n" || l == "\n" {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            hdrs.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let body = read_body(&mut reader, &hdrs)?;
    Ok(ClientResponse {
        status,
        headers: hdrs,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked() {
        let raw = b"4\r\nWiki\r\n6\r\npedia \r\n0\r\n\r\n";
        let h = vec![("Transfer-Encoding".to_string(), "chunked".to_string())];
        assert_eq!(read_body(&mut &raw[..], &h).unwrap(), b"Wikipedia ");
    }

    #[test]
    fn serve_and_request() {
        let listener = bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || {
            serve(
                listener,
                Arc::new(|r: &Request| match r.path.as_str() {
                    "/x" => Response::new(200, "text/plain", "hello"),
                    "/post" => Response::new(200, "text/plain", r.body.clone())
                        .with_header("X-Auth", r.header("authorization").unwrap_or("-")),
                    "/stream" => Response::streamed(
                        "text/plain",
                        Box::new(|w: &mut dyn Write| w.write_all(b"streamed")),
                    ),
                    _ => Response::new(404, "text/plain", ""),
                }),
            )
        });
        let r = request(&addr, "GET", "/x?a=1", &[], b"", Duration::from_secs(2)).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"hello"[..]));
        assert_eq!(r.header("content-type"), Some("text/plain"));
        let r = request(&addr, "GET", "/nope", &[], b"", Duration::from_secs(2)).unwrap();
        assert_eq!(r.status, 404);
        let r = request(
            &addr,
            "POST",
            "/post",
            &[("Authorization", "Basic eA==")],
            b"confirm=ERASE",
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"confirm=ERASE"[..]));
        assert_eq!(r.header("x-auth"), Some("Basic eA=="));
        let r = request(&addr, "GET", "/stream", &[], b"", Duration::from_secs(2)).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"streamed"[..]));
        assert_eq!(r.header("content-length"), None);
    }
}
