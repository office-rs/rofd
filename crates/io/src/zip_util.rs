use std::io::{Cursor, Read, Write};

use crate::error::OfdError;

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
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(OfdError::Io)?;
        out.push((name, buf));
    }
    Ok(out)
}

/// Write entries to a new deflate ZIP. Order preserved.
pub fn write_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, OfdError> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(name, opts).map_err(|source| OfdError::Zip {
            entry: name.clone(),
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
