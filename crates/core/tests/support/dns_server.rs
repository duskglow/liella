//! A DNS server for the tests (spec §16.4): UDP and TCP on 127.0.0.1 and a port of its own, in
//! threads of the test process, answering from a fixed zone. Nothing leaves the machine.
//!
//! Names (ASCII case ignored): the zone's records (CNAME chains followed); `nx.*` NXDOMAIN;
//! `servfail.*` SERVFAIL; `refused.*` REFUSED; `formerr.*` FORMERR; `silent.*` no answer;
//! `badid.*` a response with a wrong ID first; `wrongq.*` a response to another question;
//! `trunc.*` TC over UDP (the records over TCP); `ad.*` the AD bit set; any other name
//! NOERROR without records. Every query is logged (`proto name type ad=0|1 edns=0|1`).
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::sync::{Arc, Mutex};

/// A record: owner, type, data.
pub type Record = (&'static str, u16, Vec<u8>);

/// A running server.
pub struct Server {
    pub port: u16,
    pub log: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// The queries received so far (and forget them).
    pub fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }
}

pub fn a(owner: &'static str, ip: [u8; 4]) -> Record {
    (owner, 1, ip.to_vec())
}

pub fn aaaa(owner: &'static str, ip: std::net::Ipv6Addr) -> Record {
    (owner, 28, ip.octets().to_vec())
}

pub fn wire(name: &str) -> Vec<u8> {
    let mut w = Vec::new();
    for l in name.split('.').filter(|l| !l.is_empty()) {
        w.push(l.len() as u8);
        w.extend_from_slice(l.as_bytes());
    }
    w.push(0);
    w
}

pub fn cname(owner: &'static str, target: &str) -> Record {
    (owner, 5, wire(target))
}

pub fn ptr(owner: &'static str, target: &str) -> Record {
    (owner, 12, wire(target))
}

fn read_name(m: &[u8], mut at: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    loop {
        let n = *m.get(at)? as usize;
        if n == 0 {
            return Some((out, at + 1));
        }
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&String::from_utf8_lossy(m.get(at + 1..at + 1 + n)?));
        at += 1 + n;
    }
}

fn type_name(t: u16) -> String {
    match t {
        1 => "A".into(),
        28 => "AAAA".into(),
        12 => "PTR".into(),
        5 => "CNAME".into(),
        16 => "TXT".into(),
        _ => t.to_string(),
    }
}

/// The response to `q` (None: no answer); `tcp`: over TCP.
fn respond(zone: &[Record], q: &[u8], tcp: bool, log: &Mutex<Vec<String>>) -> Option<(Vec<u8>, bool)> {
    let (name, end) = read_name(q, 12)?;
    let qtype = u16::from_be_bytes([*q.get(end)?, *q.get(end + 1)?]);
    let edns = u16::from_be_bytes([q[10], q[11]]) > 0;
    log.lock().unwrap().push(format!("{} {} {} ad={} edns={}", if tcp { "tcp" } else { "udp" }, name, type_name(qtype), u8::from(q[3] & 0x20 != 0), u8::from(edns)));
    let lname = name.to_ascii_lowercase();
    let first = lname.split('.').next().unwrap_or("");
    let mut r = q.get(..end + 4)?.to_vec();
    r[2] = 0x84 | (q[2] & 0x01);
    r[3] = 0x80;
    r[4..12].copy_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
    let mut wrong_id_first = false;
    match first {
        "nx" => r[3] = 0x83,
        "servfail" => r[3] = 0x82,
        "refused" => r[3] = 0x85,
        "formerr" => r[3] = 0x81,
        "silent" => return None,
        "wrongq" => {
            let mut w = q[..12].to_vec();
            w[2] = 0x84 | (q[2] & 1);
            w[3] = 0x80;
            w[4..12].copy_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
            w.extend_from_slice(&wire("other.test"));
            w.extend_from_slice(&[0, 1, 0, 1]);
            return Some((w, false));
        }
        "trunc" if !tcp => {
            r[2] |= 0x02;
            return Some((r, false));
        }
        "badid" if !tcp => wrong_id_first = true,
        "ad" => r[3] |= 0x20,
        _ => {}
    }
    if r[3] & 0x0f == 0 {
        let mut target = lname.clone();
        let mut count = 0u16;
        for _ in 0..8 {
            let mut next = None;
            for (owner, t, data) in zone {
                if owner.eq_ignore_ascii_case(&target) && (*t == qtype || (*t == 5 && qtype != 5)) {
                    r.extend_from_slice(&wire(owner));
                    r.extend_from_slice(&t.to_be_bytes());
                    r.extend_from_slice(&[0, 1, 0, 0, 1, 0x2c]);
                    r.extend_from_slice(&(data.len() as u16).to_be_bytes());
                    r.extend_from_slice(data);
                    count += 1;
                    if *t == 5 && qtype != 5 {
                        next = Some(read_name(data, 0).unwrap().0);
                    }
                }
            }
            match next {
                Some(n) => target = n,
                None => break,
            }
        }
        r[6..8].copy_from_slice(&count.to_be_bytes());
    }
    Some((r, wrong_id_first))
}

/// Start a server for `zone`.
pub fn start(zone: Vec<Record>) -> Server {
    let zone = Arc::new(zone);
    let log = Arc::new(Mutex::new(Vec::new()));
    let (u, t, port) = loop {
        let u = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = u.local_addr().unwrap().port();
        if let Ok(t) = TcpListener::bind(("127.0.0.1", port)) {
            break (u, t, port);
        }
    };
    {
        let (zone, log) = (zone.clone(), log.clone());
        std::thread::spawn(move || {
            let mut q = [0u8; 4096];
            loop {
                let Ok((n, from)) = u.recv_from(&mut q) else { continue };
                if let Some((r, wrong)) = respond(&zone, &q[..n], false, &log) {
                    if wrong {
                        let mut w = r.clone();
                        w[0] ^= 0xff;
                        let _ = u.send_to(&w, from);
                    }
                    let _ = u.send_to(&r, from);
                }
            }
        });
    }
    {
        let (zone, log) = (zone.clone(), log.clone());
        std::thread::spawn(move || {
            for c in t.incoming() {
                let Ok(mut c) = c else { continue };
                let (zone, log) = (zone.clone(), log.clone());
                std::thread::spawn(move || loop {
                    let mut lb = [0u8; 2];
                    if c.read_exact(&mut lb).is_err() {
                        return;
                    }
                    let mut q = vec![0u8; u16::from_be_bytes(lb) as usize];
                    if c.read_exact(&mut q).is_err() {
                        return;
                    }
                    let Some((r, _)) = respond(&zone, &q, true, &log) else { return };
                    let _ = c.write_all(&(r.len() as u16).to_be_bytes());
                    let _ = c.write_all(&r);
                });
            }
        });
    }
    Server { port, log }
}

/// A port on 127.0.0.1 where nothing listens (UDP: connection refused).
pub fn closed_port() -> u16 {
    let u = UdpSocket::bind("127.0.0.1:0").unwrap();
    u.local_addr().unwrap().port()
}

/// A UDP port that receives and never answers (kept open for the process).
pub fn silent_port() -> u16 {
    let u = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = u.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut b = [0u8; 4096];
        loop {
            let _ = u.recv_from(&mut b);
        }
    });
    port
}
