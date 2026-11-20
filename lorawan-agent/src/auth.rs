// SPDX-License-Identifier: MIT
//! Login for the panel's eMMC actions (backups, install): HTTP Basic with a ChirpStack user's email and password,
//! checked against ChirpStack's own login (`api.InternalService/Login`). The panel keeps no accounts of its own.
//! While ChirpStack still accepts its default admin/admin, these actions are refused: anyone on the LAN would know it.

use crate::http::Request;
use crate::pb::Writer;
use crate::register::grpc_web;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

/// A good login is remembered this long, so a dozen backup downloads do not mean a dozen ChirpStack logins.
const REMEMBER: Duration = Duration::from_secs(600);
/// How long the "is admin/admin still valid" answer is kept.
const DEFAULT_CHECK: Duration = Duration::from_secs(30);

pub enum Check {
    Ok,
    /// No or wrong credentials: answer 401 so the browser asks.
    Unauthorized,
    /// ChirpStack still has its default password.
    DefaultPassword,
    /// ChirpStack did not answer.
    Unavailable(String),
}

pub struct Auth {
    api: String,
    good: Mutex<Option<(String, Instant)>>,
    default_pw: Mutex<Option<(bool, Instant)>>,
}

enum Login {
    Ok,
    Wrong,
    Unavailable(String),
}

impl Auth {
    pub fn new(api: &str) -> Auth {
        Auth {
            api: api.to_string(),
            good: Mutex::new(None),
            default_pw: Mutex::new(None),
        }
    }

    fn login(&self, email: &str, password: &str) -> Login {
        let mut req = Writer::default();
        req.string(1, email).string(2, password); // LoginRequest: email, password
        match grpc_web(&self.api, "api.InternalService/Login", "", &req.0) {
            Ok(_) => Login::Ok,
            // UNAUTHENTICATED: wrong email or password
            Err(e) if e.code == 16 => Login::Wrong,
            Err(e) => Login::Unavailable(e.to_string()),
        }
    }

    fn default_password_valid(&self) -> Result<bool, String> {
        let mut cached = self.default_pw.lock().unwrap();
        if let Some((v, at)) = *cached {
            if at.elapsed() < DEFAULT_CHECK {
                return Ok(v);
            }
        }
        let v = match self.login("admin", "admin") {
            Login::Ok => true,
            Login::Wrong => false,
            Login::Unavailable(e) => return Err(e),
        };
        *cached = Some((v, Instant::now()));
        Ok(v)
    }

    pub fn check(&self, req: &Request) -> Check {
        match self.default_password_valid() {
            Ok(true) => return Check::DefaultPassword,
            Ok(false) => {}
            Err(e) => return Check::Unavailable(e),
        }
        let Some(header) = req.header("authorization") else {
            return Check::Unauthorized;
        };
        {
            let good = self.good.lock().unwrap();
            if let Some((h, at)) = good.as_ref() {
                if h == header && at.elapsed() < REMEMBER {
                    return Check::Ok;
                }
            }
        }
        let Some((email, password)) = basic_credentials(header) else {
            return Check::Unauthorized;
        };
        match self.login(&email, &password) {
            Login::Ok => {
                *self.good.lock().unwrap() = Some((header.to_string(), Instant::now()));
                Check::Ok
            }
            Login::Wrong => {
                // Slows down guessing; one connection thread waits, the panel goes on.
                thread::sleep(Duration::from_secs(2));
                Check::Unauthorized
            }
            Login::Unavailable(e) => Check::Unavailable(e),
        }
    }
}

/// "Basic <base64(user:password)>" → (user, password).
pub fn basic_credentials(header: &str) -> Option<(String, String)> {
    let (scheme, value) = header.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = String::from_utf8(base64_decode(value.trim())?).ok()?;
    let (user, password) = decoded.split_once(':')?;
    Some((user.to_string(), password.to_string()))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        // "admin:s3cret pass" and "a:b:c" (the password may contain colons)
        assert_eq!(
            basic_credentials("Basic YWRtaW46czNjcmV0IHBhc3M="),
            Some(("admin".into(), "s3cret pass".into()))
        );
        assert_eq!(
            basic_credentials("basic YTpiOmM="),
            Some(("a".into(), "b:c".into()))
        );
        assert_eq!(basic_credentials("Bearer abc"), None);
        assert_eq!(basic_credentials("Basic !!!"), None);
    }

    #[test]
    fn base64() {
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
        assert_eq!(base64_decode("TQ==").unwrap(), b"M");
        assert_eq!(base64_decode("").unwrap(), b"");
    }
}
