use std::io::{Cursor, Read, Write};

use crate::error::OfdError;

/// How much of an entry's declared size is trusted when its buffer is
/// reserved. The size in a zip header is the writer's word for it; beyond
/// this the buffer grows with what is really read.
const RESERVE_AT_MOST: u64 = 16 * 1024 * 1024;

/// Read every entry (name + bytes) from a .ofd ZIP. Order preserved.
pub fn read_all_entries(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, OfdError> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|source| OfdError::Zip {
        entry: "<archive>".into(),
        source,
    })?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|source| OfdError::Zip {
            entry: format!("@{i}"),
            source,
        })?;
        let name = entry.name().to_string();
        let mut buf = Vec::with_capacity(entry.size().min(RESERVE_AT_MOST) as usize);
        entry.read_to_end(&mut buf).map_err(OfdError::Io)?;
        out.push((name, buf));
    }
    Ok(out)
}

/// Write entries to a new deflate ZIP. Order preserved.
pub fn write_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, OfdError> {
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    write_zip_borrowed(&borrowed)
}

/// [`write_zip`] over borrowed names and bytes, for a caller that already
/// owns the data (the preserving save hands back most of the original
/// package unchanged and should not clone it).
pub fn write_zip_borrowed(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, OfdError> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file::<_, ()>(*name, opts)
            .map_err(|source| OfdError::Zip {
                entry: (*name).to_owned(),
                source,
            })?;
        zip.write_all(bytes).map_err(OfdError::Io)?;
    }
    let cursor = zip.finish().map_err(|source| OfdError::Zip {
        entry: "<finish>".into(),
        source,
    })?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(out: &mut Vec<u8>, value: u16) {
        out.extend(value.to_le_bytes());
    }

    fn le32(out: &mut Vec<u8>, value: u32) {
        out.extend(value.to_le_bytes());
    }

    fn le64(out: &mut Vec<u8>, value: u64) {
        out.extend(value.to_le_bytes());
    }

    /// A one-entry zip that declares an uncompressed size of `u64::MAX` for
    /// an entry holding one byte. Sizes in the zip64 range travel in the
    /// extra field (id `0x0001`): uncompressed, then compressed.
    fn zip_with_a_lying_size() -> Vec<u8> {
        const CRC_OF_ONE: u32 = 0x83DC_EFB7; // CRC-32 of the byte b'1'
                                             // One deflate block, stored: six bytes that inflate to b"1".
        const DATA: [u8; 6] = [0x01, 0x01, 0x00, 0xFE, 0xFF, b'1'];
        let mut extra = Vec::new();
        le16(&mut extra, 0x0001);
        le16(&mut extra, 16);
        le64(&mut extra, u64::MAX);
        le64(&mut extra, 6);
        let extra_len = u16::try_from(extra.len()).unwrap();

        let mut out = Vec::new();
        // Local file header.
        le32(&mut out, 0x0403_4B50);
        le16(&mut out, 45); // version needed
        le16(&mut out, 0); // flags
        le16(&mut out, 8); // deflate
        le16(&mut out, 0); // time
        le16(&mut out, 0); // date
        le32(&mut out, CRC_OF_ONE);
        le32(&mut out, 0xFFFF_FFFF); // compressed size: in the extra field
        le32(&mut out, 0xFFFF_FFFF); // uncompressed size: in the extra field
        le16(&mut out, 5);
        le16(&mut out, extra_len);
        out.extend(b"a.txt");
        out.extend(&extra);
        out.extend(DATA);
        let central_at = u32::try_from(out.len()).unwrap();

        // Central directory.
        le32(&mut out, 0x0201_4B50);
        le16(&mut out, 45); // version made by
        le16(&mut out, 45); // version needed
        le16(&mut out, 0); // flags
        le16(&mut out, 8); // deflate
        le16(&mut out, 0); // time
        le16(&mut out, 0); // date
        le32(&mut out, CRC_OF_ONE);
        le32(&mut out, 0xFFFF_FFFF);
        le32(&mut out, 0xFFFF_FFFF);
        le16(&mut out, 5);
        le16(&mut out, extra_len);
        le16(&mut out, 0); // comment
        le16(&mut out, 0); // disk
        le16(&mut out, 0); // internal attributes
        le32(&mut out, 0); // external attributes
        le32(&mut out, 0); // local header offset
        out.extend(b"a.txt");
        out.extend(&extra);
        let central_size = u32::try_from(out.len()).unwrap() - central_at;

        // End of the central directory.
        le32(&mut out, 0x0605_4B50);
        le16(&mut out, 0);
        le16(&mut out, 0);
        le16(&mut out, 1);
        le16(&mut out, 1);
        le32(&mut out, central_size);
        le32(&mut out, central_at);
        le16(&mut out, 0);
        out
    }

    #[test]
    fn a_declared_size_is_not_reserved() {
        // The size in a zip header is the writer's word for it. Reserving it
        // let a few bytes ask for the whole address space (final review I4).
        let bytes = zip_with_a_lying_size();
        let read = std::panic::catch_unwind(|| {
            read_all_entries(&bytes).map_err(|error| error.to_string())
        });
        let entries = read.expect("a declared size is not reserved: reading panicked");
        let entries = entries.expect("the entry is read");
        assert_eq!(entries, vec![("a.txt".to_owned(), b"1".to_vec())]);
    }
}
