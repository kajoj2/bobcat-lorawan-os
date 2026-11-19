// SPDX-License-Identifier: MIT
//! Just enough protobuf to read ChirpStack's gateway messages and write two API requests. Field numbers come from
//! chirpstack_api 4.19 (`proto/chirpstack/gw/gw.proto`, `api/gateway.proto`, `api/tenant.proto`).

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Value<'a> {
    pub fn as_u64(self) -> Option<u64> {
        match self {
            Value::Varint(v) | Value::Fixed64(v) => Some(v),
            Value::Fixed32(v) => Some(v as u64),
            Value::Bytes(_) => None,
        }
    }

    /// int32 fields are sign-extended to 64 bits on the wire.
    pub fn as_i32(self) -> Option<i32> {
        self.as_u64().map(|v| v as i64 as i32)
    }

    pub fn as_f32(self) -> Option<f32> {
        match self {
            Value::Fixed32(v) => Some(f32::from_bits(v)),
            _ => None,
        }
    }

    pub fn as_bytes(self) -> Option<&'a [u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_str(self) -> Option<&'a str> {
        self.as_bytes().and_then(|b| std::str::from_utf8(b).ok())
    }
}

/// Iterates over the fields of one message. Stops at the first malformed field.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.buf.get(self.pos)?;
            self.pos += 1;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = (u32, Value<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let key = self.varint()?;
        let field = u32::try_from(key >> 3).ok()?;
        let value = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => Value::Fixed64(u64::from_le_bytes(self.take(8)?.try_into().ok()?)),
            2 => {
                let n = usize::try_from(self.varint()?).ok()?;
                Value::Bytes(self.take(n)?)
            }
            5 => Value::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
            _ => {
                self.pos = self.buf.len();
                return None;
            }
        };
        Some((field, value))
    }
}

/// Builds a message; fields at their default value are left out, as protobuf does.
#[derive(Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    fn varint(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.0.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.0.push(v as u8);
    }

    fn key(&mut self, field: u32, wire_type: u8) {
        self.varint(u64::from(field) << 3 | u64::from(wire_type));
    }

    pub fn uint(&mut self, field: u32, v: u64) -> &mut Self {
        if v != 0 {
            self.key(field, 0);
            self.varint(v);
        }
        self
    }

    pub fn bytes(&mut self, field: u32, b: &[u8]) -> &mut Self {
        if !b.is_empty() {
            self.key(field, 2);
            self.varint(b.len() as u64);
            self.0.extend_from_slice(b);
        }
        self
    }

    pub fn string(&mut self, field: u32, s: &str) -> &mut Self {
        self.bytes(field, s.as_bytes())
    }

    /// An embedded message is written even when empty, so its presence is kept.
    pub fn message(&mut self, field: u32, m: &[u8]) -> &mut Self {
        self.key(field, 2);
        self.varint(m.len() as u64);
        self.0.extend_from_slice(m);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut inner = Writer::default();
        inner.uint(1, 868_100_000);
        let mut w = Writer::default();
        w.string(1, "0016c001f150193b")
            .uint(2, 300)
            .message(3, &inner.0)
            .uint(4, 0);
        let fields: Vec<_> = Reader::new(&w.0).collect();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], (1, Value::Bytes(b"0016c001f150193b")));
        assert_eq!(fields[1].1.as_u64(), Some(300));
        let (_, v) = Reader::new(fields[2].1.as_bytes().unwrap()).next().unwrap();
        assert_eq!(v.as_u64(), Some(868_100_000));
    }

    #[test]
    fn negative_int32_and_float() {
        // rssi = -25 (int32, field 6) and snr = 15.5 (float, field 7) as the forwarder sends them
        let mut buf = vec![0x30];
        buf.extend_from_slice(&[0xe7, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]);
        buf.push(0x3d);
        buf.extend_from_slice(&15.5f32.to_le_bytes());
        let f: Vec<_> = Reader::new(&buf).collect();
        assert_eq!(f[0].1.as_i32(), Some(-25));
        assert_eq!(f[1].1.as_f32(), Some(15.5));
    }

    #[test]
    fn truncated_input_stops() {
        assert_eq!(Reader::new(&[0x0a, 0x05, b'a']).count(), 0);
    }
}
