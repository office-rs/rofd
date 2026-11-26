//! Minimal DER walker for io: enough to locate the picture inside an
//! electronic seal (ESS_Seal) and inside an SES signature. It only reads
//! structure; the full reader, the writer and all cryptography live in
//! rofd-sign.
//!
//! Every input is untrusted package data, so parsing is total (malformed
//! input yields `None`) and bounded (`MAX_DEPTH`, `MAX_NODES`): a damaged or
//! hostile file can neither panic nor make the parser run away.

pub(crate) const TAG_OCTET_STRING: u8 = 0x04;
pub(crate) const TAG_IA5_STRING: u8 = 0x16;
pub(crate) const TAG_SEQUENCE: u8 = 0x30;

/// Deepest nesting the walker follows.
pub(crate) const MAX_DEPTH: usize = 16;
/// Most nodes the walker reads for one input.
pub(crate) const MAX_NODES: usize = 4096;

/// One tag-length-value node of a DER structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tlv<'a> {
    pub tag: u8,
    /// The content octets.
    pub content: &'a [u8],
    /// The whole node: header plus content.
    pub raw: &'a [u8],
    /// Offset just past the node, within the input `read_tlv` was given.
    pub end: usize,
}

/// Read the TLV that starts at `offset`.
pub(crate) fn read_tlv(bytes: &[u8], offset: usize) -> Option<Tlv<'_>> {
    let tag = *bytes.get(offset)?;
    // High-tag-number form does not occur in the structures read here.
    if tag & 0x1f == 0x1f {
        return None;
    }
    let first = *bytes.get(offset + 1)?;
    let (length, header) = if first & 0x80 == 0 {
        (usize::from(first), 2)
    } else {
        let octets = usize::from(first & 0x7f);
        // 0 is the indefinite length (not DER); more than 4 is absurd here.
        if octets == 0 || octets > 4 {
            return None;
        }
        let start = offset + 2;
        let mut length = 0usize;
        for &octet in bytes.get(start..start + octets)? {
            length = (length << 8) | usize::from(octet);
            if length > bytes.len() {
                return None;
            }
        }
        (length, 2 + octets)
    };
    let content_start = offset + header;
    let content_end = content_start.checked_add(length)?;
    Some(Tlv {
        tag,
        content: bytes.get(content_start..content_end)?,
        raw: bytes.get(offset..content_end)?,
        end: content_end,
    })
}

/// The consecutive TLVs that fill `content`, reading at most `budget` nodes.
fn read_all<'a>(content: &'a [u8], budget: &mut usize) -> Option<Vec<Tlv<'a>>> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < content.len() {
        let node = read_tlv(content, offset)?;
        *budget = budget.checked_sub(1)?;
        offset = node.end;
        out.push(node);
    }
    Some(out)
}

/// A DER node with its children; a primitive node has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node<'a> {
    pub tag: u8,
    pub content: &'a [u8],
    pub raw: &'a [u8],
    pub children: Vec<Node<'a>>,
}

/// Parse the first TLV of `bytes` into a tree. Trailing bytes are ignored.
pub(crate) fn parse_tree(bytes: &[u8]) -> Option<Node<'_>> {
    let mut budget = MAX_NODES;
    let first = read_tlv(bytes, 0)?;
    budget = budget.checked_sub(1)?;
    build(first, 0, &mut budget)
}

fn build<'a>(tlv: Tlv<'a>, depth: usize, budget: &mut usize) -> Option<Node<'a>> {
    let node = |children| Node {
        tag: tlv.tag,
        content: tlv.content,
        raw: tlv.raw,
        children,
    };
    // Bit 0x20 marks a constructed (container) tag.
    if tlv.tag & 0x20 == 0 {
        return Some(node(Vec::new()));
    }
    if depth >= MAX_DEPTH {
        return None;
    }
    let children = read_all(tlv.content, budget)?
        .into_iter()
        .map(|child| build(child, depth + 1, budget))
        .collect::<Option<Vec<_>>>()?;
    Some(node(children))
}

/// The value of a DER INTEGER that is a u64, or `None`.
pub(crate) fn unsigned_integer(content: &[u8]) -> Option<u64> {
    if content.len() > 8 {
        return None;
    }
    let significant = content
        .iter()
        .position(|&octet| octet != 0)
        .map_or(&[][..], |at| &content[at..]);
    // A set sign bit means a negative number.
    if significant.first().is_some_and(|octet| octet & 0x80 != 0) {
        return None;
    }
    significant.iter().try_fold(0u64, |value, &octet| {
        value
            .checked_mul(256)
            .and_then(|value| value.checked_add(u64::from(octet)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG_INTEGER: u8 = 0x02;
    const TAG_BIT_STRING: u8 = 0x03;

    fn seq(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x30];
        match content.len() {
            n if n < 0x80 => out.push(n as u8),
            n if n < 0x100 => {
                out.push(0x81);
                out.push(n as u8);
            }
            n => {
                out.push(0x82);
                out.push((n >> 8) as u8);
                out.push(n as u8);
            }
        }
        out.extend_from_slice(content);
        out
    }

    fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, content.len() as u8];
        out.extend_from_slice(content);
        out
    }

    #[test]
    fn reads_a_nested_structure() {
        let picture = seq(&[
            tlv(TAG_IA5_STRING, b"PNG"),
            tlv(TAG_OCTET_STRING, &[0x89, 0x50]),
            tlv(TAG_INTEGER, &[40]),
            tlv(TAG_INTEGER, &[0x01, 0x00]),
        ]
        .concat());
        let seal = seq(&[
            seq(&[tlv(TAG_INTEGER, &[1]), tlv(TAG_INTEGER, &[4])].concat()),
            tlv(TAG_INTEGER, &[7]),
            seq(&[]),
            picture,
        ]
        .concat());
        // ESS_Seal ::= SEQUENCE { eSealInfo, certificate, algorithm, signature }
        let ess_seal = seq(&[
            seal,
            tlv(TAG_OCTET_STRING, b"certificate"),
            tlv(TAG_BIT_STRING, &[0]),
        ]
        .concat());
        let root = parse_tree(&ess_seal).expect("parses");
        assert_eq!(root.tag, TAG_SEQUENCE);
        assert_eq!(root.raw.len(), ess_seal.len(), "raw covers the whole node");
        assert_eq!(root.children.len(), 3);
        let info = &root.children[0];
        assert_eq!(info.children.len(), 4);
        assert_eq!(info.children[3].children[0].content, b"PNG");
        assert_eq!(
            unsigned_integer(info.children[3].children[2].content),
            Some(40)
        );
        assert_eq!(
            unsigned_integer(info.children[3].children[3].content),
            Some(256),
            "a leading zero is dropped"
        );
    }

    #[test]
    fn every_prefix_of_a_structure_is_some_or_none() {
        let seal = seq(&[
            seq(&[]),
            seq(&[tlv(TAG_INTEGER, &[1]), tlv(TAG_BIT_STRING, &[0, 1, 2])].concat()),
        ]
        .concat());
        for end in 0..=seal.len() {
            let _ = parse_tree(&seal[..end]);
        }
        assert!(parse_tree(&seal).is_some());
        assert!(parse_tree(&seal[..seal.len() - 1]).is_none(), "truncated");
    }

    #[test]
    fn rejects_indefinite_length_and_high_tag_numbers() {
        assert!(
            read_tlv(&[0x30, 0x80, 0x00, 0x00], 0).is_none(),
            "indefinite length"
        );
        assert!(
            read_tlv(&[0x1F, 0x81, 0x02, 0x00], 0).is_none(),
            "high tag number"
        );
        assert!(
            read_tlv(&[0x30, 0x90, 0x00], 0).is_none(),
            "reserved length form"
        );
        assert!(
            read_tlv(&[0x30, 0x05, 0x00], 0).is_none(),
            "length past the end"
        );
        assert!(read_tlv(&[], 0).is_none());
        assert!(unsigned_integer(&[0x80]).is_none(), "negative");
        assert!(unsigned_integer(&[0; 9]).is_none(), "longer than u64");
    }

    #[test]
    fn depth_and_node_budgets_are_enforced() {
        let mut deep = tlv(TAG_INTEGER, &[1]);
        for _ in 0..(MAX_DEPTH + 2) {
            deep = seq(&deep);
        }
        assert!(parse_tree(&deep).is_none(), "too deep");
        let mut wide = Vec::new();
        for _ in 0..(MAX_NODES + 1) {
            wide.extend_from_slice(&tlv(TAG_INTEGER, &[1]));
        }
        assert!(
            {
                let mut budget = MAX_NODES;
                read_all(&wide, &mut budget)
            }
            .is_none(),
            "too many nodes"
        );
    }

    #[test]
    fn children_cover_the_content_exactly() {
        let content = [tlv(TAG_INTEGER, &[1]), tlv(TAG_INTEGER, &[2, 3])].concat();
        let mut budget = MAX_NODES;
        let kids = read_all(&content, &mut budget).expect("children");
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[1].end, content.len());
        assert!(
            {
                let mut budget = MAX_NODES;
                read_all(&content[..content.len() - 1], &mut budget)
            }
            .is_none(),
            "a truncated last node"
        );
    }
}
