pub mod annotation;
pub mod annotation_entry;
pub mod body;
pub mod package;
pub mod resource;

use rofd_dom::RawFragment;

/// One merged child in an interleaved (known + passthrough) sequence.
pub(crate) enum Child<'a> {
    /// Modelled child, by index into the known children.
    Known(usize),
    /// Captured passthrough fragment.
    Raw(&'a RawFragment),
}

/// Merge a parent's known children (implicit indexes `0..known_count`) with
/// captured raw fragments at interleaved positions (spec 2026-11-11 D3):
/// a raw's `position` is the number of children that preceded it in the
/// original file, so slot `s` holds the raw whose position equals `s`, and
/// the known children fill the remaining slots in order.
pub(crate) fn interleave<'a>(known_count: usize, raws: &'a [RawFragment]) -> Vec<Child<'a>> {
    let mut sorted: Vec<&RawFragment> = raws.iter().collect();
    sorted.sort_by_key(|r| r.position);
    let mut out = Vec::with_capacity(known_count + sorted.len());
    let mut ri = 0;
    let mut k = 0;
    for slot in 0..(known_count + sorted.len()) {
        if ri < sorted.len() && sorted[ri].position == slot {
            out.push(Child::Raw(sorted[ri]));
            ri += 1;
        } else if k < known_count {
            out.push(Child::Known(k));
            k += 1;
        }
    }
    out
}
