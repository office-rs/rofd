//! Unknown-content capture (spec 2026-11-11 D3).
//!
//! When the page/document parsers meet an element they do not model, the
//! subtree's original source bytes are captured verbatim (with inherited
//! namespace declarations synthesized into the fragment root, so writers can
//! re-emit the string byte-for-byte) and stored as a
//! [`rofd_dom::RawFragment`].

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::error::OfdError;

/// In-scope namespace declarations. `start` is called for every element
/// Start (it records how many `xmlns:*` attributes that element declared),
/// `end` for every element End; the stack therefore always mirrors the
/// open-element chain.
#[derive(Default)]
pub(crate) struct NsScope {
    stack: Vec<(String, String)>,
    open: Vec<usize>,
}

impl NsScope {
    pub(crate) fn start(&mut self, e: &BytesStart) {
        let mut n = 0;
        for a in e.attributes().flatten() {
            let key = String::from_utf8_lossy(a.key.as_ref());
            if let Some(prefix) = key.strip_prefix("xmlns:") {
                n += 1;
                self.stack.push((
                    prefix.to_string(),
                    String::from_utf8_lossy(&a.value).into_owned(),
                ));
            }
        }
        self.open.push(n);
    }

    pub(crate) fn end(&mut self) {
        if let Some(n) = self.open.pop() {
            for _ in 0..n {
                self.stack.pop();
            }
        }
    }

    /// Namespace declarations the fragment needs: prefixes it uses (element
    /// or attribute names) that are in scope here but not declared on the
    /// fragment's own root tag.
    fn needed_by(&self, fragment: &str) -> Vec<(String, String)> {
        let mut used: Vec<String> = Vec::new();
        let mut root_declared: Vec<String> = Vec::new();
        let mut reader = Reader::from_str(fragment);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut first = true;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    if let Some((p, _)) = name.split_once(':') {
                        used.push(p.to_string());
                    }
                    for a in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                        if let Some(prefix) = key.strip_prefix("xmlns:") {
                            if first {
                                root_declared.push(prefix.to_string());
                            }
                        } else if let Some((p, _)) = key.split_once(':') {
                            used.push(p.to_string());
                        }
                    }
                    first = false;
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
        let mut needed: Vec<(String, String)> = Vec::new();
        for (p, uri) in &self.stack {
            if used.contains(p) && !root_declared.contains(p) && !needed.iter().any(|(q, _)| q == p)
            {
                needed.push((p.clone(), uri.clone()));
            }
        }
        needed
    }

    /// Binding for a prefix currently in scope, if any.
    fn binding_of(&self, prefix: &str) -> Option<&(String, String)> {
        self.stack.iter().rev().find(|(p, _)| p == prefix)
    }

    /// Inject the needed declarations into the fragment's root start tag.
    fn synthesize_into(&self, raw: &mut String) {
        let needed = self.needed_by(raw);
        if needed.is_empty() {
            return;
        }
        let close = raw.find('>').expect("fragment root start tag has '>'");
        // Self-closing root (`/>`): insert before the slash so the tag stays
        // self-closing; otherwise insert before the closing '>'.
        let insert_at = if raw[..close].ends_with('/') {
            close - 1
        } else {
            close
        };
        let decls: String = needed
            .iter()
            .map(|(p, u)| format!(" xmlns:{p}=\"{u}\""))
            .collect();
        raw.insert_str(insert_at, &decls);
    }
}

/// Capture the raw source of an unknown subtree whose Start event has
/// already been consumed. `start_pos` is the reader's byte offset BEFORE
/// that Start event was read; `xml` is the full document being parsed.
/// Reads forward through the balanced subtree and returns its source text
/// with inherited namespaces synthesized into the root tag.
pub(crate) fn capture_raw_subtree(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    xml: &str,
    start_pos: usize,
    ns: &NsScope,
    entry: &str,
) -> Result<String, OfdError> {
    let mut depth = 1usize;
    let end_pos;
    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    end_pos = reader.buffer_position() as usize;
                    break;
                }
            }
            Ok(Event::Eof) => {
                return Err(OfdError::Schema {
                    entry: entry.into(),
                    reason: "unterminated unknown element".into(),
                });
            }
            Err(e) => {
                return Err(OfdError::Xml {
                    entry: entry.into(),
                    loc: String::new(),
                    source: e,
                });
            }
            _ => {}
        }
    }
    // Trim leading whitespace: with trim_text(true) the reader may have
    // skipped indentation between the previous event and this subtree, and
    // pos_before anchors before that whitespace - the element itself starts
    // at '<'.
    let mut raw = xml[start_pos..end_pos].trim_start().to_string();
    ns.synthesize_into(&mut raw);
    Ok(raw)
}

/// Capture a self-closing unknown element (`Event::Empty`): its bytes span
/// [`start_pos`, the current reader position).
pub(crate) fn capture_raw_empty(
    reader: &Reader<&[u8]>,
    xml: &str,
    start_pos: usize,
    ns: &NsScope,
) -> String {
    // Same leading-whitespace trim as `capture_raw_subtree`.
    let mut raw = xml[start_pos..reader.buffer_position() as usize]
        .trim_start()
        .to_string();
    ns.synthesize_into(&mut raw);
    raw
}

/// Collect attributes outside the known set for a recognized element
/// (passthrough, spec D3 attribute layer). `xmlns` declarations are scope,
/// not attributes, and never land here.
pub(crate) fn raw_attrs_of(e: &BytesStart, known: &[&[u8]], ns: &NsScope) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for a in e.attributes().flatten() {
        let key = a.key.as_ref();
        if known.contains(&key) || key.starts_with(b"xmlns") {
            continue;
        }
        out.push((
            String::from_utf8_lossy(key).into_owned(),
            String::from_utf8_lossy(&a.value).into_owned(),
        ));
    }
    // A prefixed attribute would dangle after re-emission (the declaration
    // lives on an ancestor). Synthesize the needed bindings as xmlns pseudo
    // attributes at the end of the list - written on the object, they keep
    // the output well-formed and re-establish scope on re-parse.
    let mut bindings: Vec<(String, String)> = Vec::new();
    for (k, _) in &out {
        if let Some((prefix, _)) = k.split_once(':') {
            if let Some((_, uri)) = ns.binding_of(prefix) {
                let decl = (format!("xmlns:{prefix}"), uri.clone());
                if !bindings.contains(&decl) && !out.contains(&decl) {
                    bindings.push(decl);
                }
            }
        }
    }
    out.extend(bindings);
    out
}
