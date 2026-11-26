//! DER reader and writer for the signing line.
//!
//! The reader is flat on purpose: a node keeps the raw bytes of its whole
//! TLV, so the to-be-signed part of a signature can be verified against the
//! bytes exactly as they are in the file. That matters because a signature
//! is computed over those bytes, and re-encoding a structure can change
//! them. The writer produces DER for the structures rofd-sign creates.

use crate::error::SignError;

pub const TAG_BOOLEAN: u8 = 0x01;
pub const TAG_INTEGER: u8 = 0x02;
pub const TAG_BIT_STRING: u8 = 0x03;
pub const TAG_OCTET_STRING: u8 = 0x04;
pub const TAG_NULL: u8 = 0x05;
pub const TAG_OID: u8 = 0x06;
pub const TAG_UTF8_STRING: u8 = 0x0C;
pub const TAG_PRINTABLE_STRING: u8 = 0x13;
pub const TAG_IA5_STRING: u8 = 0x16;
pub const TAG_UTC_TIME: u8 = 0x17;
pub const TAG_GENERALIZED_TIME: u8 = 0x18;
pub const TAG_SEQUENCE: u8 = 0x30;
pub const TAG_SET: u8 = 0x31;

/// One tag-length-value node of a DER structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerNode<'a> {
    pub tag: u8,
    /// The whole node: header plus content.
    pub raw: &'a [u8],
    /// The content octets.
    pub content: &'a [u8],
}

fn malformed(what: &str) -> SignError {
    SignError::InvalidInput {
        reason: format!("malformed DER: {what}"),
    }
}

/// Read exactly one node: the whole of `bytes` must be that node.
pub fn parse_one(bytes: &[u8]) -> Result<DerNode<'_>, SignError> {
    let (node, next) = read_tlv(bytes, 0)?;
    if next != bytes.len() {
        return Err(malformed("trailing bytes after the structure"));
    }
    Ok(node)
}

/// The consecutive nodes that make up `node`'s content.
pub fn parse_children<'a>(node: &DerNode<'a>) -> Result<Vec<DerNode<'a>>, SignError> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < node.content.len() {
        let (child, next) = read_tlv(node.content, offset)?;
        offset = next;
        out.push(child);
    }
    Ok(out)
}

fn read_tlv(bytes: &[u8], offset: usize) -> Result<(DerNode<'_>, usize), SignError> {
    let tag = *bytes.get(offset).ok_or_else(|| malformed("missing tag"))?;
    if tag & 0x1f == 0x1f {
        return Err(malformed("high-tag-number form is not supported"));
    }
    let first = *bytes
        .get(offset + 1)
        .ok_or_else(|| malformed("missing length"))?;
    let (length, header) = if first & 0x80 == 0 {
        (usize::from(first), 2)
    } else {
        let octets = usize::from(first & 0x7f);
        // 0 is the indefinite length, which DER does not allow.
        if octets == 0 || octets > 4 {
            return Err(malformed("unsupported length form"));
        }
        let start = offset + 2;
        let mut length = 0usize;
        for &octet in bytes
            .get(start..start + octets)
            .ok_or_else(|| malformed("truncated length"))?
        {
            length = (length << 8) | usize::from(octet);
            if length > bytes.len() {
                return Err(malformed("length past the end"));
            }
        }
        (length, 2 + octets)
    };
    let content_start = offset + header;
    let content_end = content_start
        .checked_add(length)
        .ok_or_else(|| malformed("length overflow"))?;
    let content = bytes
        .get(content_start..content_end)
        .ok_or_else(|| malformed("truncated content"))?;
    Ok((
        DerNode {
            tag,
            raw: &bytes[offset..content_end],
            content,
        },
        content_end,
    ))
}

impl DerNode<'_> {
    /// The value of an INTEGER.
    pub fn as_i64(&self) -> Result<i64, SignError> {
        if self.tag != TAG_INTEGER {
            return Err(malformed("expected an INTEGER"));
        }
        let bytes = self.content;
        if bytes.is_empty() || bytes.len() > 8 {
            return Err(malformed("INTEGER is empty or wider than 8 bytes"));
        }
        // Two's complement, big-endian: start from the sign and shift the
        // bytes in. Every prefix of at most 8 bytes fits an i64.
        let mut value: i64 = if bytes[0] & 0x80 != 0 { -1 } else { 0 };
        for &octet in bytes {
            value = (value << 8) | i64::from(octet);
        }
        Ok(value)
    }

    /// The object identifier in dotted form (for example `1.2.156.10197.1.501`).
    pub fn as_oid(&self) -> Result<String, SignError> {
        if self.tag != TAG_OID {
            return Err(malformed("expected an OBJECT IDENTIFIER"));
        }
        if self.content.is_empty() {
            return Err(malformed("empty OBJECT IDENTIFIER"));
        }
        let mut subidentifiers = Vec::new();
        let mut value: u64 = 0;
        for (at, &octet) in self.content.iter().enumerate() {
            if value > u64::MAX >> 7 {
                return Err(malformed("OBJECT IDENTIFIER arc is too large"));
            }
            value = (value << 7) | u64::from(octet & 0x7f);
            if octet & 0x80 == 0 {
                subidentifiers.push(value);
                value = 0;
            } else if at == self.content.len() - 1 {
                return Err(malformed("OBJECT IDENTIFIER ends mid-arc"));
            }
        }
        let first = subidentifiers[0];
        let (arc0, arc1) = if first < 40 {
            (0, first)
        } else if first < 80 {
            (1, first - 40)
        } else {
            (2, first - 80)
        };
        let mut dotted = format!("{arc0}.{arc1}");
        for subidentifier in &subidentifiers[1..] {
            dotted.push('.');
            dotted.push_str(&subidentifier.to_string());
        }
        Ok(dotted)
    }

    /// The text of a string or time node, whatever its string type.
    pub fn as_text(&self) -> Result<String, SignError> {
        if !matches!(
            self.tag,
            TAG_UTF8_STRING
                | TAG_PRINTABLE_STRING
                | TAG_IA5_STRING
                | TAG_UTC_TIME
                | TAG_GENERALIZED_TIME
        ) {
            return Err(malformed("expected a string"));
        }
        Ok(String::from_utf8_lossy(self.content).into_owned())
    }

    /// The bytes of a BIT STRING, without its leading unused-bit count.
    pub fn as_bit_string(&self) -> Result<Vec<u8>, SignError> {
        if self.tag != TAG_BIT_STRING {
            return Err(malformed("expected a BIT STRING"));
        }
        let (unused, bytes) = self
            .content
            .split_first()
            .ok_or_else(|| malformed("BIT STRING without its unused-bit count"))?;
        if *unused > 7 {
            return Err(malformed("BIT STRING unused-bit count is out of range"));
        }
        Ok(bytes.to_vec())
    }
}

/// One node into `out`: the tag, the shortest length that holds the content,
/// the content. A length from 128 up is written as `0x80 | n` followed by the
/// `n` lowest-order octets of the length, with `n` as large as the length
/// needs: a 16 MiB picture needs four, not three.
pub(crate) fn push_tlv(out: &mut Vec<u8>, tag: u8, content: &[u8]) {
    out.push(tag);
    let length = content.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let octets = (usize::BITS - length.leading_zeros()).div_ceil(8);
        out.push(0x80 | octets as u8);
        for at in (0..octets).rev() {
            out.push((length >> (8 * at)) as u8);
        }
    }
    out.extend_from_slice(content);
}

/// Builds DER, outermost node first.
#[derive(Debug, Clone, Default)]
pub struct DerWriter {
    buf: Vec<u8>,
}

impl DerWriter {
    pub fn new() -> Self {
        Self::default()
    }

    fn tlv(&mut self, tag: u8, content: &[u8]) -> &mut Self {
        push_tlv(&mut self.buf, tag, content);
        self
    }

    /// A constructed node built by `body`.
    fn with(&mut self, tag: u8, body: impl FnOnce(&mut DerWriter)) -> &mut Self {
        let mut inner = DerWriter::new();
        body(&mut inner);
        self.tlv(tag, &inner.buf)
    }

    /// An INTEGER in minimal two's complement.
    pub fn integer(&mut self, value: i64) -> &mut Self {
        let bytes = value.to_be_bytes();
        let mut start = 0;
        while start + 1 < bytes.len() {
            let (current, next) = (bytes[start], bytes[start + 1]);
            let redundant =
                (current == 0x00 && next & 0x80 == 0) || (current == 0xFF && next & 0x80 != 0);
            if redundant {
                start += 1;
            } else {
                break;
            }
        }
        self.tlv(TAG_INTEGER, &bytes[start..])
    }

    /// A BIT STRING holding whole bytes.
    pub fn bit_string(&mut self, bytes: &[u8]) -> &mut Self {
        let mut content = Vec::with_capacity(bytes.len() + 1);
        content.push(0);
        content.extend_from_slice(bytes);
        self.tlv(TAG_BIT_STRING, &content)
    }

    pub fn octet_string(&mut self, bytes: &[u8]) -> &mut Self {
        self.tlv(TAG_OCTET_STRING, bytes)
    }

    /// An OBJECT IDENTIFIER given in dotted form.
    ///
    /// # Panics
    ///
    /// If `dotted` is not a dotted list of at least two numbers. The callers
    /// pass literals, so that is a programming error.
    pub fn oid(&mut self, dotted: &str) -> &mut Self {
        let arcs: Vec<u64> = dotted
            .split('.')
            .map(|arc| arc.parse().expect("malformed object identifier"))
            .collect();
        assert!(arcs.len() >= 2, "malformed object identifier: {dotted}");
        let mut content = Vec::new();
        base_128(arcs[0] * 40 + arcs[1], &mut content);
        for arc in &arcs[2..] {
            base_128(*arc, &mut content);
        }
        self.tlv(TAG_OID, &content)
    }

    pub fn ia5_string(&mut self, text: &str) -> &mut Self {
        debug_assert!(text.is_ascii(), "IA5String must be ASCII");
        self.tlv(TAG_IA5_STRING, text.as_bytes())
    }

    pub fn printable_string(&mut self, text: &str) -> &mut Self {
        self.tlv(TAG_PRINTABLE_STRING, text.as_bytes())
    }

    pub fn utf8_string(&mut self, text: &str) -> &mut Self {
        self.tlv(TAG_UTF8_STRING, text.as_bytes())
    }

    /// A UTCTime, given as `YYMMDDHHMMSSZ`.
    pub fn utc_time(&mut self, text: &str) -> &mut Self {
        self.tlv(TAG_UTC_TIME, text.as_bytes())
    }

    /// A GeneralizedTime, given as `YYYYMMDDHHMMSSZ`.
    pub fn generalized_time(&mut self, text: &str) -> &mut Self {
        self.tlv(TAG_GENERALIZED_TIME, text.as_bytes())
    }

    /// A SEQUENCE built by `body`.
    pub fn sequence(&mut self, body: impl FnOnce(&mut DerWriter)) -> &mut Self {
        self.with(TAG_SEQUENCE, body)
    }

    /// A SET built by `body`.
    pub fn set(&mut self, body: impl FnOnce(&mut DerWriter)) -> &mut Self {
        self.with(TAG_SET, body)
    }

    /// A SET tagged `[n]` implicitly: the SET with its tag replaced by
    /// the context tag, which is how CMS-style structures mark optional
    /// sets such as `certificates`.
    pub fn implicit_set(&mut self, number: u8, body: impl FnOnce(&mut DerWriter)) -> &mut Self {
        self.with(0xA0 | number, body)
    }

    /// A constructed `[n]` context-specific node built by `body`.
    pub fn explicit(&mut self, number: u8, body: impl FnOnce(&mut DerWriter)) -> &mut Self {
        self.with(0xA0 | number, body)
    }

    pub fn null(&mut self) -> &mut Self {
        self.tlv(TAG_NULL, &[])
    }

    pub fn boolean(&mut self, value: bool) -> &mut Self {
        self.tlv(TAG_BOOLEAN, &[u8::from(value)])
    }

    /// Append bytes that are already encoded, to embed a structure verbatim
    /// (the certificate signature covers the encoded to-be-signed part).
    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    /// The encoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

/// Append `value` in base 128: seven bits per octet, the high bit set on
/// every octet but the last.
fn base_128(value: u64, out: &mut Vec<u8>) {
    let bits = u64::BITS - value.max(1).leading_zeros();
    let octets = (bits as usize).div_ceil(7);
    for i in (0..octets).rev() {
        let mut octet = ((value >> (7 * i)) & 0x7f) as u8;
        if i > 0 {
            octet |= 0x80;
        }
        out.push(octet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(mut build: impl FnMut(&mut DerWriter)) -> Vec<u8> {
        let mut writer = DerWriter::new();
        build(&mut writer);
        writer.into_bytes()
    }

    #[test]
    fn writer_round_trips_through_reader() {
        let bytes = sample(|w| {
            w.sequence(|w| {
                w.integer(300);
                w.oid("1.2.156.10197.1.501");
                w.octet_string(&[0xDE, 0xAD]);
                w.bit_string(&[0x01, 0x02]);
                w.generalized_time("20261124000000Z");
                w.utc_time("261124000000Z");
                w.ia5_string("PNG");
                w.utf8_string("签名");
                w.set(|w| {
                    w.integer(1);
                    w.integer(2);
                });
                w.explicit(0, |w| {
                    w.integer(2);
                });
                w.null();
                w.boolean(true);
            });
        });
        let node = parse_one(&bytes).expect("the whole input is one TLV");
        assert_eq!(node.tag, TAG_SEQUENCE);
        assert_eq!(node.raw.len(), bytes.len(), "raw covers the whole node");
        let kids = parse_children(&node).expect("children");
        assert_eq!(kids.len(), 12);
        assert_eq!(kids[0].as_i64().unwrap(), 300);
        assert_eq!(kids[1].as_oid().unwrap(), "1.2.156.10197.1.501");
        assert_eq!(kids[2].tag, TAG_OCTET_STRING);
        assert_eq!(kids[2].content, &[0xDE, 0xAD]);
        assert_eq!(kids[3].as_bit_string().unwrap(), vec![0x01, 0x02]);
        assert_eq!(kids[4].as_text().unwrap(), "20261124000000Z");
        assert_eq!(kids[5].as_text().unwrap(), "261124000000Z");
        assert_eq!(kids[6].as_text().unwrap(), "PNG");
        assert_eq!(kids[7].as_text().unwrap(), "签名");
        let set = parse_children(&kids[8]).expect("set children");
        assert_eq!(
            (
                set.len(),
                set[0].as_i64().unwrap(),
                set[1].as_i64().unwrap()
            ),
            (2, 1, 2)
        );
        assert_eq!(kids[9].tag, 0xA0, "[0] EXPLICIT");
        let inner = parse_children(&kids[9]).expect("explicit children");
        assert_eq!(inner[0].as_i64().unwrap(), 2);
        assert_eq!(kids[10].tag, TAG_NULL);
        assert_eq!(kids[11].tag, TAG_BOOLEAN);
    }

    #[test]
    fn parse_one_rejects_trailing_bytes_and_truncated_input() {
        let bytes = sample(|w| {
            w.integer(7);
        });
        assert!(parse_one(&bytes).is_ok());
        assert!(
            parse_one(&[bytes.as_slice(), &[0]].concat()).is_err(),
            "trailing byte"
        );
        assert!(parse_one(&bytes[..1]).is_err(), "truncated");
        assert!(parse_one(&[]).is_err());
    }

    #[test]
    fn oid_encoding_covers_large_arcs() {
        // 1.2.840.113549.1.7.1 is the standard DER example.
        let bytes = sample(|w| {
            w.oid("1.2.840.113549.1.7.1");
        });
        assert_eq!(
            bytes,
            [0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x01]
        );
        assert_eq!(
            parse_one(&bytes).unwrap().as_oid().unwrap(),
            "1.2.840.113549.1.7.1"
        );
    }

    #[test]
    fn integer_handles_the_i64_range() {
        for value in [0_i64, -1, 1, 127, 128, i64::MAX, i64::MIN] {
            let bytes = sample(|w| {
                w.integer(value);
            });
            assert_eq!(parse_one(&bytes).unwrap().as_i64().unwrap(), value);
        }
    }

    #[test]
    fn raw_embeds_preencoded_bytes() {
        let inner = sample(|w| {
            w.integer(7);
        });
        let bytes = sample(|w| {
            w.sequence(|w| {
                w.raw(&inner);
                w.integer(1);
            });
        });
        let node = parse_one(&bytes).unwrap();
        let kids = parse_children(&node).unwrap();
        assert_eq!(
            kids[0].raw,
            inner.as_slice(),
            "the embedded bytes are kept verbatim"
        );
        assert_eq!(kids[1].as_i64().unwrap(), 1);
    }

    #[test]
    fn accessors_reject_the_wrong_tag_or_content() {
        let integer = sample(|w| {
            w.integer(1);
        });
        let node = parse_one(&integer).unwrap();
        assert!(node.as_text().is_err(), "an INTEGER is not text");
        assert!(node.as_oid().is_err());
        assert!(node.as_bit_string().is_err());
        let too_long = [0x02, 0x09, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        assert!(
            parse_one(&too_long).unwrap().as_i64().is_err(),
            "wider than i64"
        );
        let bad_oid = [0x06, 0x01, 0x80];
        assert!(
            parse_one(&bad_oid).unwrap().as_oid().is_err(),
            "an unfinished base-128 arc"
        );
    }

    #[test]
    fn a_long_length_is_written_whole() {
        // From 16 MiB on a length needs a fourth octet. The writer used to
        // drop the top byte there: a 16 MiB node came out as one of 5 bytes.
        for length in [
            0xFF,
            0x100,
            0xFFFF,
            0x10000,
            0xFF_FFFF,
            0x100_0000,
            0x100_0001,
            0x0100_0100,
        ] {
            let mut writer = DerWriter::new();
            writer.octet_string(&vec![0u8; length]);
            let bytes = writer.into_bytes();
            let octets =
                usize::try_from((usize::BITS - length.leading_zeros()).div_ceil(8)).unwrap();
            let expected: Vec<u8> = std::iter::once(0x80 | octets as u8)
                .chain((0..octets).rev().map(|at| (length >> (8 * at)) as u8))
                .collect();
            assert_eq!(bytes[1..1 + octets + 1], expected[..], "length {length:#x}");
            let node = parse_one(&bytes).expect("the reader takes what the writer wrote");
            assert_eq!(node.content.len(), length, "length {length:#x}");
        }
    }
}
