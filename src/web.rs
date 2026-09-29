//! Small loopback-only development preview, not a public HTTP deployment server.
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use crate::{events, Frame, render};

struct Request {
    method: String,
    path: String,
    host: String,
    origin: Option<String>,
    length: usize,
    content_type: String,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid HTTP request")
}

fn parse_header(bytes: &[u8]) -> io::Result<Request> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut lines = text.split("\r\n");
    let parts: Vec<_> = lines.next().ok_or_else(invalid)?.split_whitespace().collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" { return Err(invalid()); }
    let mut request = Request { method: parts[0].into(), path: parts[1].into(),
        host: String::new(), origin: None, length: 0, content_type: String::new() };
    let mut length_seen = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once(':').ok_or_else(invalid)?;
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "host" if request.host.is_empty() => request.host = value.into(),
            "origin" if request.origin.is_none() => request.origin = Some(value.into()),
            "content-type" if request.content_type.is_empty() => request.content_type = value.into(),
            "content-length" if !length_seen => {
                request.length = value.parse().map_err(|_| invalid())?;
                length_seen = true;
            }
            "transfer-encoding" | "host" | "origin" | "content-type" | "content-length" => return Err(invalid()),
            _ => {}
        }
    }
    if request.length > 64 { return Err(invalid()); }
    Ok(request)
}

fn read_request(stream: &mut TcpStream) -> io::Result<(Request, Vec<u8>)> {
    stream.set_read_timeout(Some(Duration::from_millis(150)))?;
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut parsed = None;
    loop {
        if Instant::now() >= deadline || bytes.len() > 8192 { return Err(invalid()); }
        if parsed.is_none() {
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                parsed = Some((parse_header(&bytes[..end])?, end + 4));
            }
        }
        if let Some((request, offset)) = parsed.as_ref() {
            if bytes.len() >= offset + request.length {
                let body = bytes[*offset..offset + request.length].to_vec();
                return Ok((parsed.unwrap().0, body));
            }
        }
        let count = stream.read(&mut chunk)?;
        if count == 0 { return Err(invalid()); }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) -> io::Result<()> {
    stream.set_write_timeout(Some(Duration::from_millis(300)))?;
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'\r\n\r\n", body.len())?;
    stream.write_all(body)
}

fn frame_json(frame: &Frame) -> String {
    let grid = render(frame, 100, 30);
    let mut text = String::new();
    for cell in &grid.cells {
        match cell.ch {
            '\\' => text.push_str("\\\\"),
            '"' => text.push_str("\\\""),
            c => text.push(c),
        }
    }
    let colors: Vec<_> = grid.cells.iter().map(|cell| {
        let (r, g, b) = cell.fg;
        ((r as u32) << 16 | (g as u32) << 8 | b as u32).to_string()
    }).collect();
    let event = frame.event.unwrap_or(events::Message { source: "NONE", state: "DISCONNECTED" });
    format!("{{\"cols\":100,\"rows\":30,\"text\":\"{text}\",\"colors\":[{}],\"source\":\"{}\",\"state\":\"{}\"}}", colors.join(","), event.source, event.state)
}

pub fn run(port: u16, seconds: Option<f32>) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let host = format!("127.0.0.1:{}", listener.local_addr()?.port());
    let origin = format!("http://{host}");
    println!("Open {origin} — LOCAL PREVIEW, no model connected. Ctrl-C stops the server.");
    io::stdout().flush()?;
    let started = Instant::now();
    let mut last = started;
    let mut state = events::State::new();
    let mut frame = Frame::new();
    loop {
        if seconds.is_some_and(|limit| started.elapsed().as_secs_f32() >= limit) { break; }
        let now = Instant::now();
        if now.duration_since(last) >= Duration::from_millis(33) {
            frame.step_event(now.duration_since(last).as_secs_f32().min(0.1), state.snapshot(now), state.motion(now));
            last = now;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let Ok((request, body)) = read_request(&mut stream) else {
                    let _ = respond(&mut stream, "400 Bad Request", "text/plain", b"Invalid request");
                    continue;
                };
                // Same-origin POST only, no CORS. Avoid exposing localhost controls to other sites.
                if request.host != host || (request.origin.is_some() && request.origin.as_deref() != Some(&origin)) {
                    let _ = respond(&mut stream, "403 Forbidden", "text/plain", b"Origin/host rejected");
                    continue;
                }
                let result = match (request.method.as_str(), request.path.as_str()) {
                    ("GET", "/") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", include_bytes!("../web/index.html")),
                    ("GET", "/app.js") => respond(&mut stream, "200 OK", "text/javascript; charset=utf-8", include_bytes!("../web/app.js")),
                    ("GET", "/chat-feedback.js") => respond(&mut stream, "200 OK", "text/javascript; charset=utf-8", include_bytes!("../web/chat-feedback.js")),
                    ("GET", "/style.css") => respond(&mut stream, "200 OK", "text/css", include_bytes!("../web/style.css")),
                    ("GET", "/frame") => respond(&mut stream, "200 OK", "application/json", frame_json(&frame).as_bytes()),
                    ("POST", "/event") if request.origin.as_deref() == Some(&origin) && request.content_type == "text/plain;charset=UTF-8" => {
                        let message = std::str::from_utf8(&body).ok().and_then(|text| events::Message::parse(text).ok());
                        if let Some(message) = message {
                            state.receive(message, Instant::now());
                            respond(&mut stream, "200 OK", "text/plain", b"OK")
                        } else { respond(&mut stream, "400 Bad Request", "text/plain", b"Invalid event") }
                    }
                    ("POST", "/event") => respond(&mut stream, "403 Forbidden", "text/plain", b"Same-origin event required"),
                    _ => respond(&mut stream, "404 Not Found", "text/plain", b"Not found"),
                };
                // Browser navigation or cancellation may close a connection mid-response.
                if let Err(error) = result { eprintln!("preview response failed: {}", error.kind()); }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_ambiguous_or_large_requests() {
        for request in [
            "POST /event HTTP/1.1\r\nContent-Length: 65",
            "POST /event HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 3",
            "POST /event HTTP/1.1\r\nTransfer-Encoding: chunked",
        ] { assert!(parse_header(request.as_bytes()).is_err()); }
    }
    #[test]
    fn browser_frame_contains_only_grid_and_status() {
        let json = frame_json(&Frame::new());
        assert!(json.starts_with("{\"cols\":100,\"rows\":30,"));
        assert!(json.contains("\"source\":\"NONE\""));
        assert!(json.contains("\\\\")); // escaped rider slashes
    }
}
