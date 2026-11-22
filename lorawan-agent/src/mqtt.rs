// SPDX-License-Identifier: MIT
//! A minimal MQTT 3.1.1 client (QoS 0, clean session) for the local Mosquitto: enough to follow what the MQTT
//! Forwarder and ChirpStack publish, and to publish the agent's own device alerts and BLE sensor readings.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

pub struct Publish {
    pub topic: String,
    pub payload: Vec<u8>,
}

pub struct Client {
    stream: TcpStream,
    keepalive: Duration,
    /// The broker drops a client that sends nothing for 1.5 keepalive periods, however much it receives, so
    /// pings go by the time of the last packet sent, not of the last one received.
    last_sent: Instant,
    ping_sent: Option<Instant>,
}

fn put_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn packet(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    let mut len = body.len();
    loop {
        let mut b = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            b |= 0x80;
        }
        out.push(b);
        if len == 0 {
            break;
        }
    }
    out.extend_from_slice(body);
    out
}

/// How often an idle read wakes up to check whether a ping is due.
const POLL: Duration = Duration::from_secs(5);

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl Client {
    pub fn connect(addr: &str, client_id: &str, keepalive: Duration) -> io::Result<Client> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(POLL))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let mut c = Client {
            stream,
            keepalive,
            last_sent: Instant::now(),
            ping_sent: None,
        };

        let mut body = Vec::new();
        put_str(&mut body, "MQTT");
        body.push(4); // protocol level 3.1.1
        body.push(0x02); // clean session
        body.extend_from_slice(&(keepalive.as_secs() as u16).to_be_bytes());
        put_str(&mut body, client_id);
        c.stream.write_all(&packet(0x10, &body))?;

        let (kind, body) = c.read_packet()?;
        if kind >> 4 != 2 || body.len() < 2 {
            return Err(bad("no CONNACK"));
        }
        if body[1] != 0 {
            return Err(bad(&format!("connection refused, code {}", body[1])));
        }
        Ok(c)
    }

    pub fn subscribe(&mut self, topics: &[&str]) -> io::Result<()> {
        let mut body = vec![0, 1]; // packet id 1
        for t in topics {
            put_str(&mut body, t);
            body.push(0); // QoS 0
        }
        self.send(&packet(0x82, &body))
    }

    /// PUBLISH at QoS 0; `retain` keeps it on the broker for later subscribers.
    pub fn publish(&mut self, topic: &str, payload: &[u8], retain: bool) -> io::Result<()> {
        let mut body = Vec::with_capacity(topic.len() + payload.len() + 2);
        put_str(&mut body, topic);
        body.extend_from_slice(payload);
        self.send(&packet(0x30 | u8::from(retain), &body))
    }

    /// Answers the broker's pings for a client that only publishes.
    pub fn keep_up(&mut self) -> io::Result<()> {
        self.keep_alive()?;
        self.stream.set_nonblocking(true)?;
        let mut buf = [0u8; 64];
        let r = self.stream.read(&mut buf);
        self.stream.set_nonblocking(false)?;
        match r {
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "broker closed the connection",
            )),
            Ok(n) => {
                if buf[..n].contains(&0xd0) {
                    self.ping_sent = None;
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn send(&mut self, p: &[u8]) -> io::Result<()> {
        self.stream.write_all(p)?;
        self.last_sent = Instant::now();
        Ok(())
    }

    /// PINGREQ when half the keepalive has passed since the last packet sent; an error when the last ping got
    /// no answer within the keepalive period.
    fn keep_alive(&mut self) -> io::Result<()> {
        if let Some(t) = self.ping_sent {
            if t.elapsed() > self.keepalive {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "broker did not answer PINGREQ",
                ));
            }
        } else if self.last_sent.elapsed() >= self.keepalive / 2 {
            self.send(&[0xc0, 0x00])?;
            self.ping_sent = Some(Instant::now());
        }
        Ok(())
    }

    fn read_packet(&mut self) -> io::Result<(u8, Vec<u8>)> {
        let mut head = [0u8; 1];
        self.stream.read_exact(&mut head)?;
        let mut len = 0usize;
        for i in 0..4 {
            let mut b = [0u8; 1];
            self.stream.read_exact(&mut b)?;
            len |= usize::from(b[0] & 0x7f) << (7 * i);
            if b[0] & 0x80 == 0 {
                let mut body = vec![0u8; len];
                self.stream.read_exact(&mut body)?;
                return Ok((head[0], body));
            }
        }
        Err(bad("bad remaining length"))
    }

    /// Waits for the next PUBLISH, keeping the connection alive on the way. An error means the connection is gone.
    pub fn next_publish(&mut self) -> io::Result<Publish> {
        loop {
            self.keep_alive()?;
            let (kind, body) = match self.read_packet() {
                Ok(p) => p,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue
                }
                Err(e) => return Err(e),
            };
            match kind >> 4 {
                13 => self.ping_sent = None, // PINGRESP
                3 => {
                    let qos = (kind >> 1) & 3;
                    if body.len() < 2 {
                        return Err(bad("short PUBLISH"));
                    }
                    let tlen = usize::from(u16::from_be_bytes([body[0], body[1]]));
                    let mut rest = 2 + tlen;
                    let topic = body.get(2..rest).ok_or_else(|| bad("short topic"))?;
                    let topic = String::from_utf8_lossy(topic).into_owned();
                    if qos > 0 {
                        rest += 2; // packet id; we subscribe with QoS 0 so this does not happen
                    }
                    let payload = body.get(rest..).unwrap_or_default().to_vec();
                    return Ok(Publish { topic, payload });
                }
                _ => {} // SUBACK
            }
        }
    }
}

/// A publishing connection that comes back by itself: each publish reconnects when the last one failed.
pub struct Publisher {
    broker: String,
    client_id: String,
    client: Option<Client>,
}

impl Publisher {
    pub fn new(broker: &str, client_id: &str) -> Publisher {
        Publisher {
            broker: broker.to_string(),
            client_id: client_id.to_string(),
            client: None,
        }
    }

    pub fn publish(&mut self, topic: &str, payload: &[u8], retain: bool) -> io::Result<()> {
        if self.client.is_none() {
            self.client = Some(Client::connect(
                &self.broker,
                &self.client_id,
                Duration::from_secs(60),
            )?);
        }
        let c = self.client.as_mut().expect("connected above");
        let r = c.keep_up().and_then(|_| c.publish(topic, payload, retain));
        if r.is_err() {
            self.client = None;
        }
        r
    }

    /// Keeps an idle connection alive (call every few seconds).
    pub fn idle(&mut self) {
        if let Some(c) = self.client.as_mut() {
            if c.keep_up().is_err() {
                self.client = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// The bug seen on the board: with messages arriving more often than the keepalive, the client never pinged
    /// and Mosquitto dropped it after 90 s. A busy broker must still get PINGREQs.
    #[test]
    fn pings_while_messages_keep_coming() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let broker = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).unwrap(); // CONNECT
            s.write_all(&[0x20, 0x02, 0x00, 0x00]).unwrap(); // CONNACK
            s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(3) {
                let mut body = vec![0, 1, b't'];
                body.extend_from_slice(b"x");
                s.write_all(&packet(0x30, &body)).unwrap();
                if let Ok(n) = s.read(&mut buf) {
                    if buf[..n].contains(&0xc0) {
                        s.write_all(&[0xd0, 0x00]).unwrap(); // PINGRESP
                        return true;
                    }
                }
            }
            false
        });
        let mut c = Client::connect(&addr, "t", Duration::from_secs(2)).unwrap();
        for _ in 0..100 {
            if c.next_publish().is_err() || broker.is_finished() {
                break;
            }
        }
        assert!(
            broker.join().unwrap(),
            "no PINGREQ while messages kept coming"
        );
    }

    #[test]
    fn publish_packet() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let broker = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).unwrap(); // CONNECT
            s.write_all(&[0x20, 0x02, 0x00, 0x00]).unwrap();
            let n = s.read(&mut buf).unwrap();
            buf[..n].to_vec()
        });
        let mut p = Publisher::new(&addr, "t");
        p.publish("a/b", b"hi", true).unwrap();
        assert_eq!(
            broker.join().unwrap(),
            vec![0x31, 7, 0, 3, b'a', b'/', b'b', b'h', b'i']
        );
    }

    #[test]
    fn remaining_length_encoding() {
        assert_eq!(&packet(0xc0, &[])[..], &[0xc0, 0x00]);
        let p = packet(0x30, &[0u8; 321]);
        assert_eq!(&p[..3], &[0x30, 0xc1, 0x02]);
        assert_eq!(p.len(), 3 + 321);
    }
}
