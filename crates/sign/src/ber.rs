//! BER to DER: rewrite the encoding a PKCS#12 file uses when it was written
//! the older way.
//!
//! Several CA tools write certificate files as BER with indefinite lengths
//! and with strings split into segments (`24 80 04 82 .. .. 00 00`). The
//! DER readers rofd-sign relies on accept none of that. This pass rewrites
//! such a file into the equivalent DER: definite, minimal lengths
//! everywhere and each string as one primitive value. It changes encoding
//! only, never content.

/// Deepest nesting accepted.
const MAX_DEPTH: usize = 64;
/// The length of an indefinite-length node.
const INDEFINITE: usize = usize::MAX;
/// The universal OCTET STRING: the one string PKCS#12 splits into segments.
const TAG_OCTET_STRING: u8 = 0x04;

/// Rewrite BER as DER. `None` when the bytes are not BER: truncated, missing
/// an end marker, trailing bytes, nesting too deep.
pub(crate) fn to_der(bytes: &[u8]) -> Option<Vec<u8>> {
    let (der, used) = rewrite(bytes, 0, 0)?;
    (used == bytes.len()).then_some(der)
}

/// Rewrite the node that starts at `at`: its DER, and the offset just past
/// it.
fn rewrite(bytes: &[u8], at: usize, depth: usize) -> Option<(Vec<u8>, usize)> {
    if depth > MAX_DEPTH {
        return None;
    }
    let tag = *bytes.get(at)?;
    // 0x00 is an end marker here; high tag numbers do not occur in PKCS#12.
    if tag == 0x00 || tag & 0x1f == 0x1f {
        return None;
    }
    let constructed = tag & 0x20 != 0;
    let first = *bytes.get(at + 1)?;
    let (length, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else if first == 0x80 {
        if !constructed {
            return None;
        }
        (INDEFINITE, 2)
    } else {
        let octets = usize::from(first & 0x7f);
        if octets > 4 {
            return None;
        }
        let start = at + 2;
        let mut length = 0usize;
        for &octet in bytes.get(start..start + octets)? {
            length = (length << 8) | usize::from(octet);
            if length > bytes.len() {
                return None;
            }
        }
        (length, 2 + octets)
    };
    let content_start = at + header;
    if !constructed {
        let end = content_start.checked_add(length)?;
        return Some((encode(tag, bytes.get(content_start..end)?), end));
    }

    // A constructed node: rewrite every child.
    let content_end = (length != INDEFINITE)
        .then(|| content_start.checked_add(length))
        .flatten();
    let mut parts = Vec::new();
    let mut cursor = content_start;
    loop {
        if content_end.is_some_and(|end| cursor >= end) {
            break;
        }
        let at_end_marker = length == INDEFINITE
            && bytes.get(cursor) == Some(&0)
            && bytes.get(cursor + 1) == Some(&0);
        if at_end_marker {
            cursor += 2;
            break;
        }
        let (part, next) = rewrite(bytes, cursor, depth + 1)?;
        parts.push(part);
        cursor = next;
    }
    if content_end.is_some_and(|end| cursor != end) {
        return None;
    }
    // A string written in segments becomes one primitive string.
    if tag & 0xC0 == 0 && tag & 0x1f == TAG_OCTET_STRING {
        let mut content = Vec::new();
        for part in &parts {
            content.extend_from_slice(content_of(part)?);
        }
        return Some((encode(tag & !0x20, &content), cursor));
    }
    Some((encode(tag, &parts.concat()), cursor))
}

/// A node with the shortest correct length encoding.
fn encode(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 8);
    crate::der::push_tlv(&mut out, tag, content);
    out
}

/// The content of a node written by [`encode`].
fn content_of(node: &[u8]) -> Option<&[u8]> {
    let first = *node.get(1)?;
    let header = if first < 0x80 {
        2
    } else {
        2 + usize::from(first & 0x7f)
    };
    node.get(header..)
}

/// A PKCS#12 file rewritten as DER, with what is needed to check it.
pub(crate) struct Rewritten {
    /// The file as DER, without its integrity-check data: the check covers
    /// the original encoding, so the reader must not try to redo it.
    pub der: Vec<u8>,
    /// What the integrity check covers: the authenticated safe exactly as
    /// the file held it.
    pub auth_safe_original: Vec<u8>,
    /// The integrity-check data (MacData, DER), when the file has it.
    pub mac: Option<Vec<u8>>,
}

/// Rewrite a PKCS#12 file as DER.
///
/// Besides the encoding of the file itself, one level is nested: the
/// authenticated safe, a `SEQUENCE OF ContentInfo`, is carried inside an
/// OCTET STRING and older tools write it as BER too. A generic rewrite
/// cannot see into a string, so this function opens that one string and
/// rewrites what it holds.
///
/// `None` when the bytes are not a PKCS#12 file.
pub(crate) fn pkcs12_to_der(bytes: &[u8]) -> Option<Rewritten> {
    use crate::der::{self, TAG_OCTET_STRING};
    const OID_PKCS7_DATA: &str = "1.2.840.113549.1.7.1";
    let outer = to_der(bytes)?;
    // PFX ::= SEQUENCE { version INTEGER, authSafe ContentInfo, macData OPTIONAL }.
    let pfx = der::parse_one(&outer).ok()?;
    let pfx_kids = der::parse_children(&pfx).ok()?;
    let auth_safe = pfx_kids.get(1)?;
    // ContentInfo ::= SEQUENCE { contentType OID, content [0] EXPLICIT }.
    let content_info = der::parse_children(auth_safe).ok()?;
    let [content_type, content, ..] = content_info.as_slice() else {
        return None;
    };
    if !content_type.as_oid().is_ok_and(|oid| oid == OID_PKCS7_DATA) {
        return None;
    }
    // The [0] holds one OCTET STRING whose content is the authenticated safe.
    let inner = match der::parse_one(content.content) {
        Ok(inner) if inner.tag == TAG_OCTET_STRING => inner,
        _ => return None,
    };
    // Content already in DER comes back unchanged.
    let rewritten_content = to_der(inner.content)?;
    let content = encode(0xA0, &encode(TAG_OCTET_STRING, &rewritten_content));
    let mut body = pfx_kids[0].raw.to_vec();
    body.extend_from_slice(&encode(
        0x30,
        &[content_type.raw, content.as_slice()].concat(),
    ));
    Some(Rewritten {
        der: encode(0x30, &body),
        auth_safe_original: inner.content.to_vec(),
        mac: pfx_kids.get(2).map(|kid| kid.raw.to_vec()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indefinite_lengths_become_definite() {
        // SEQUENCE(indefinite) { INTEGER 3, SEQUENCE(indefinite) { } } EOC
        let ber = [
            0x30, 0x80, 0x02, 0x01, 0x03, 0x30, 0x80, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(
            to_der(&ber).unwrap(),
            vec![0x30, 0x05, 0x02, 0x01, 0x03, 0x30, 0x00]
        );
    }

    #[test]
    fn a_segmented_string_becomes_one_primitive_string() {
        // [0](indefinite) { OCTET STRING(indefinite) { "ab", "c" } }
        let ber = [
            0xA0, 0x80, 0x24, 0x80, 0x04, 0x02, b'a', b'b', 0x04, 0x01, b'c', 0x00, 0x00, 0x00,
            0x00,
        ];
        assert_eq!(
            to_der(&ber).unwrap(),
            vec![0xA0, 0x05, 0x04, 0x03, b'a', b'b', b'c']
        );
    }

    #[test]
    fn lengths_are_written_minimally_and_der_is_left_alone() {
        // A length that need not be long form: SEQUENCE { INTEGER 1 }.
        let non_minimal = [0x30, 0x81, 0x03, 0x02, 0x01, 0x01];
        assert_eq!(
            to_der(&non_minimal).unwrap(),
            vec![0x30, 0x03, 0x02, 0x01, 0x01]
        );
        let der = vec![0x30, 0x03, 0x02, 0x01, 0x01];
        assert_eq!(to_der(&der).unwrap(), der);
        let big = [0x04, 0x82, 0x01, 0x00]; // length 256, content added below
        let mut bytes = big.to_vec();
        bytes.extend(std::iter::repeat_n(0x5A, 256));
        assert_eq!(
            to_der(&bytes).unwrap(),
            bytes,
            "a long form that is needed stays"
        );
    }

    #[test]
    fn a_long_length_is_encoded_whole() {
        // From 16 MiB on the length needs a fourth octet (final review C2).
        for length in [0xFF_FFFFusize, 0x100_0000, 0x100_0001] {
            let bytes = encode(0x04, &vec![0u8; length]);
            let octets =
                usize::try_from((usize::BITS - length.leading_zeros()).div_ceil(8)).unwrap();
            let expected: Vec<u8> = std::iter::once(0x80 | octets as u8)
                .chain((0..octets).rev().map(|at| (length >> (8 * at)) as u8))
                .collect();
            assert_eq!(
                &bytes[1..1 + octets + 1],
                &expected[..],
                "length {length:#x}"
            );
            assert_eq!(bytes.len(), 2 + octets + length, "length {length:#x}");
        }
    }

    #[test]
    fn the_old_certificate_file_is_rewritten_into_der() {
        let pfx = std::fs::read("tests/fixtures/USER.p12").expect("fixture");
        assert_eq!(
            &pfx[..2],
            &[0x30, 0x80],
            "the fixture is indefinite-length BER"
        );
        let rewritten = pkcs12_to_der(&pfx).expect("rewritten");
        // It now reads as one DER structure.
        assert_eq!(crate::der::parse_one(&rewritten.der).unwrap().tag, 0x30);
        assert!(rewritten.der[1] & 0x80 != 0, "a long length is needed");
        // The integrity-check data is taken out for the caller, and what it
        // covers is kept as the file held it.
        assert!(rewritten.mac.is_some());
        assert!(rewritten.auth_safe_original.starts_with(&[0x30, 0x80]));
        // Content is untouched: the DER reader reads the key and certificate.
        use keystore_core::KeyStoreFormat as _;
        let store = keystore_pkcs12::Pkcs12Store::parse(
            &rewritten.der,
            &keystore_core::Password::new("777777"),
        )
        .expect("the rewritten file parses");
        let keystore = store.to_key_store();
        assert_eq!(keystore.keys().count(), 1);
        assert_eq!(
            keystore
                .certificate_chain(keystore.keys().next().unwrap())
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn damaged_input_is_rejected_never_panicking() {
        assert!(to_der(&[]).is_none());
        assert!(to_der(&[0x30]).is_none());
        assert!(
            to_der(&[0x30, 0x80, 0x02, 0x01, 0x01]).is_none(),
            "no end marker"
        );
        assert!(to_der(&[0x30, 0x05, 0x02, 0x01]).is_none(), "truncated");
        assert!(
            to_der(&[0x30, 0x02, 0x02, 0x01, 0x01]).is_none(),
            "trailing byte"
        );
        assert!(to_der(&[0x30, 0x82, 0x01]).is_none(), "truncated length");
        assert!(
            to_der(&[0x1F, 0x81, 0x02, 0x00]).is_none(),
            "high tag numbers"
        );
        assert!(
            to_der(&[0x04, 0x80, 0x00, 0x00]).is_none(),
            "a primitive cannot be indefinite"
        );
        let pfx = std::fs::read("tests/fixtures/USER.p12").expect("fixture");
        for end in 0..pfx.len() {
            let _ = to_der(&pfx[..end]);
        }
        let mut deep = vec![0x02, 0x01, 0x01];
        for _ in 0..100 {
            let mut wrapped = vec![0x30, 0x80];
            wrapped.extend_from_slice(&deep);
            wrapped.extend_from_slice(&[0x00, 0x00]);
            deep = wrapped;
        }
        assert!(to_der(&deep).is_none(), "too deep");
    }
}
