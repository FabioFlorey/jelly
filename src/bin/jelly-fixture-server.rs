//! Loopback-only HTTP fixture server for browser-state tests.
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};
const FAST: &[u8] = b"jelly-download-fixture\n";
const SLOW: &[u8] = &[b'x'; 65536];
fn serve(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request.split_whitespace().nth(1).unwrap_or("");
    match path {
        "/" => {
            let html=b"<!doctype html><meta charset=\"utf-8\"><title>Browser state fixture</title>\n<a id=\"fast\" download href=\"/fast\">Fast download</a>\n<a id=\"slow\" download href=\"/slow\">Slow download</a>\n";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                html.len()
            )?;
            stream.write_all(html)?;
        }
        "/fast" => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=\"fixture.txt\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                FAST.len()
            )?;
            stream.write_all(FAST)?;
        }
        "/slow" => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"slow.bin\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                SLOW.len() * 256
            )?;
            for _ in 0..256 {
                if stream.write_all(SLOW).is_err() {
                    break;
                }
                let _ = stream.flush();
                thread::sleep(Duration::from_millis(100));
            }
        }
        _ => {
            stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
        }
    }
    Ok(())
}
fn main() -> std::io::Result<()> {
    let portfile = std::env::args().nth(1).ok_or(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "usage: jelly-fixture-server PORT_FILE",
    ))?;
    let server = TcpListener::bind("127.0.0.1:0")?;
    fs::write(portfile, server.local_addr()?.port().to_string())?;
    for client in server.incoming() {
        match client {
            Ok(conn) => {
                thread::spawn(move || {
                    let _ = serve(conn);
                });
            }
            Err(e) => eprintln!("fixture connection error: {e}"),
        }
    }
    Ok(())
}
