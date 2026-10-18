//! A tiny authoritative DNS responder for the local TLD.
//!
//! Answers `A` with `127.0.0.1` and `AAAA` with `::1` for the TLD and every
//! name below it, `NOERROR` with no answers for other record types, and
//! `REFUSED` for anything outside the TLD. It listens on a high loopback
//! port, so it runs as the user; the OS is pointed at it by a resolver file
//! (`/etc/resolver/<tld>` on macOS) that contains that port.

use std::net::{Ipv4Addr, Ipv6Addr};

use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;
const CLASS_IN: u16 = 1;
const RCODE_NOERROR: u16 = 0;
const RCODE_FORMERR: u16 = 1;
const RCODE_NOTIMP: u16 = 4;
const RCODE_REFUSED: u16 = 5;
const TTL_SECS: u32 = 60;

/// Bind the responder on `127.0.0.1:port` (0 = any free port).
pub async fn bind(port: u16) -> std::io::Result<(UdpSocket, u16)> {
    let socket = UdpSocket::bind(("127.0.0.1", port)).await?;
    let port = socket.local_addr()?.port();
    Ok((socket, port))
}

/// Answer queries until `cancel` fires.
pub async fn serve(socket: UdpSocket, tld: String, cancel: CancellationToken) {
    let mut buf = [0u8; 1500];
    loop {
        let (len, peer) = tokio::select! {
            () = cancel.cancelled() => return,
            received = socket.recv_from(&mut buf) => match received {
                Ok(pair) => pair,
                Err(err) => {
                    tracing::debug!(%err, "DNS receive failed");
                    continue;
                }
            },
        };
        if let Some(reply) = answer(&buf[..len], &tld)
            && let Err(err) = socket.send_to(&reply, peer).await
        {
            tracing::debug!(%err, "DNS send failed");
        }
    }
}

/// Build the response to one query, or `None` if it should be ignored
/// (too short to even echo an id, or not a query).
pub fn answer(query: &[u8], tld: &str) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    let id = [query[0], query[1]];
    let flags = u16::from_be_bytes([query[2], query[3]]);
    if flags & 0x8000 != 0 {
        return None; // a response, not a query
    }
    let opcode = (flags >> 11) & 0xf;
    let recursion_desired = flags & 0x0100;
    let qdcount = u16::from_be_bytes([query[4], query[5]]);

    let reply = |rcode: u16, question: &[u8], answer: Option<(u16, Vec<u8>)>| {
        let flags = 0x8000 | (opcode << 11) | 0x0400 | recursion_desired | rcode;
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(&id);
        out.extend_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&u16::from(!question.is_empty()).to_be_bytes());
        out.extend_from_slice(&u16::from(answer.is_some()).to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]); // no authority, no additional
        out.extend_from_slice(question);
        if let Some((rtype, rdata)) = answer {
            out.extend_from_slice(&[0xc0, 0x0c]); // pointer to the question name
            out.extend_from_slice(&rtype.to_be_bytes());
            out.extend_from_slice(&CLASS_IN.to_be_bytes());
            out.extend_from_slice(&TTL_SECS.to_be_bytes());
            out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
            out.extend_from_slice(&rdata);
        }
        out
    };

    if opcode != 0 {
        return Some(reply(RCODE_NOTIMP, &[], None));
    }
    if qdcount != 1 {
        return Some(reply(RCODE_FORMERR, &[], None));
    }
    let Some((name, end)) = read_name(query, 12) else {
        return Some(reply(RCODE_FORMERR, &[], None));
    };
    if query.len() < end + 4 {
        return Some(reply(RCODE_FORMERR, &[], None));
    }
    let question = &query[12..end + 4];
    let qtype = u16::from_be_bytes([query[end], query[end + 1]]);
    let qclass = u16::from_be_bytes([query[end + 2], query[end + 3]]);

    let in_tld = name == tld || name.ends_with(&format!(".{tld}"));
    if !in_tld || qclass != CLASS_IN {
        return Some(reply(RCODE_REFUSED, question, None));
    }
    let record = match qtype {
        TYPE_A => Some((TYPE_A, Ipv4Addr::LOCALHOST.octets().to_vec())),
        TYPE_AAAA => Some((TYPE_AAAA, Ipv6Addr::LOCALHOST.octets().to_vec())),
        _ => None,
    };
    Some(reply(RCODE_NOERROR, question, record))
}

/// Read an uncompressed name starting at `pos`. Returns the lowercase name
/// without the trailing dot, and the offset just past it.
fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    loop {
        let len = *msg.get(pos)? as usize;
        pos += 1;
        if len == 0 {
            break;
        }
        if len & 0xc0 != 0 {
            return None; // compression is not used in questions
        }
        let label = msg.get(pos..pos + len)?;
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        pos += len;
        if labels.len() > 127 {
            return None;
        }
    }
    Some((labels.join("."), pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a standard query for `name` and `qtype`, with RD set.
    pub(crate) fn query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
        let mut q = Vec::new();
        q.extend_from_slice(&id.to_be_bytes());
        q.extend_from_slice(&0x0100u16.to_be_bytes());
        q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&CLASS_IN.to_be_bytes());
        q
    }

    fn rcode(reply: &[u8]) -> u16 {
        u16::from_be_bytes([reply[2], reply[3]]) & 0xf
    }

    fn ancount(reply: &[u8]) -> u16 {
        u16::from_be_bytes([reply[6], reply[7]])
    }

    #[test]
    fn answers_a_and_aaaa_inside_the_tld() {
        let q = query(0xbeef, "Blog.Test", TYPE_A);
        let reply = answer(&q, "test").unwrap();
        assert_eq!(&reply[..2], &[0xbe, 0xef]);
        assert_eq!(reply[2] & 0x80, 0x80, "QR set");
        assert_eq!(reply[2] & 0x04, 0x04, "AA set");
        assert_eq!(rcode(&reply), RCODE_NOERROR);
        assert_eq!(ancount(&reply), 1);
        assert_eq!(&reply[reply.len() - 4..], &[127, 0, 0, 1]);

        let reply = answer(&query(1, "api.blog.test", TYPE_AAAA), "test").unwrap();
        assert_eq!(ancount(&reply), 1);
        assert_eq!(&reply[reply.len() - 16..], &Ipv6Addr::LOCALHOST.octets());
    }

    #[test]
    fn other_types_get_an_empty_answer() {
        let reply = answer(&query(1, "blog.test", 15 /* MX */), "test").unwrap();
        assert_eq!(rcode(&reply), RCODE_NOERROR);
        assert_eq!(ancount(&reply), 0);
    }

    #[test]
    fn refuses_names_outside_the_tld() {
        for name in ["example.com", "nottest", "blogtest"] {
            let reply = answer(&query(1, name, TYPE_A), "test").unwrap();
            assert_eq!(rcode(&reply), RCODE_REFUSED, "{name}");
            assert_eq!(ancount(&reply), 0);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(answer(&[1, 2, 3], "test").is_none());
        let mut response = query(1, "blog.test", TYPE_A);
        response[2] |= 0x80;
        assert!(answer(&response, "test").is_none());
        let truncated = &query(1, "blog.test", TYPE_A)[..16];
        assert_eq!(rcode(&answer(truncated, "test").unwrap()), RCODE_FORMERR);
    }
}
