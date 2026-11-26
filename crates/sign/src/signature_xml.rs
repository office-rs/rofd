//! The signature description file (Signature.xml) rofd-sign writes
//! (GB/T 33190 §18.2).
//!
//! The element order follows the standard; the compact form follows the
//! layout the ofdrw samples use. The file ends without a newline, because
//! its exact bytes are hashed into the signature.

use rofd_dom::Rect;

use crate::error::SignError;

/// `sm2sign-with-sm3` (GM/T 0006): the signature method.
pub const SIGNATURE_METHOD_SM2_SM3: &str = "1.2.156.10197.1.501";

/// One `StampAnnot`: where a seal appears.
pub struct StampRef<'a> {
    pub id: u64,
    /// The page ID as written in Document.xml.
    pub page_id: &'a str,
    /// Page-local millimetres.
    pub boundary: Rect,
}

/// One protected file: its path and the base64 of its SM3 digest.
#[derive(Debug, Clone)]
pub struct Reference {
    pub file_ref: String,
    pub check_value_base64: String,
}

/// What goes into a Signature.xml.
pub struct SignatureXmlInput<'a> {
    /// Loc of the signature value file.
    pub signed_value_loc: &'a str,
    pub provider_name: &'a str,
    /// Written as an empty attribute when absent.
    pub provider_company: Option<&'a str>,
    /// Written as an empty attribute when absent.
    pub provider_version: Option<&'a str>,
    /// `YYYY-MM-DD HH:mm:ss` (a `T` separator is accepted and written as a
    /// space).
    pub date_time: &'a str,
    /// Loc of the seal file. `None` for a signature without a seal.
    pub seal_base_loc: Option<&'a str>,
    pub stamps: &'a [StampRef<'a>],
    pub references: &'a [Reference],
}

/// Serialize a Signature.xml.
pub fn signature_xml(input: &SignatureXmlInput) -> String {
    let date_time = input.date_time.replace('T', " ");
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Signature xmlns:ofd=\"http://www.ofdspec.org/2016\">",
    );
    xml.push_str("<ofd:SignedValue>");
    xml.push_str(&escape(input.signed_value_loc));
    xml.push_str("</ofd:SignedValue><ofd:SignedInfo>");
    xml.push_str(&format!(
        "<ofd:Provider ProviderName=\"{}\" Company=\"{}\" Version=\"{}\"/>",
        escape(input.provider_name),
        escape(input.provider_company.unwrap_or("")),
        escape(input.provider_version.unwrap_or(""))
    ));
    xml.push_str("<ofd:SignatureMethod>");
    xml.push_str(SIGNATURE_METHOD_SM2_SM3);
    xml.push_str("</ofd:SignatureMethod>");
    xml.push_str(&format!(
        "<ofd:SignatureDateTime>{}</ofd:SignatureDateTime>",
        escape(&date_time)
    ));
    if let Some(loc) = input.seal_base_loc {
        xml.push_str(&format!(
            "<ofd:Seal><ofd:BaseLoc>{}</ofd:BaseLoc></ofd:Seal>",
            escape(loc)
        ));
    }
    for stamp in input.stamps {
        let b = stamp.boundary;
        xml.push_str(&format!(
            "<ofd:StampAnnot ID=\"{}\" Boundary=\"{} {} {} {}\" PageRef=\"{}\"/>",
            stamp.id,
            b.x,
            b.y,
            b.w,
            b.h,
            escape(stamp.page_id)
        ));
    }
    xml.push_str("<ofd:References CheckMethod=\"SM3\">");
    for reference in input.references {
        xml.push_str(&format!(
            "<ofd:Reference FileRef=\"{}\"><ofd:CheckValue>{}</ofd:CheckValue></ofd:Reference>",
            escape(&reference.file_ref),
            reference.check_value_base64.trim()
        ));
    }
    xml.push_str("</ofd:References></ofd:SignedInfo></ofd:Signature>");
    xml
}

/// Escape the characters that matter in an attribute value or in text.
pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn digits(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn bad_time(date_time: &str) -> SignError {
    SignError::InvalidInput {
        reason: format!("not a time in the form YYYY-MM-DD HH:mm:ss: {date_time}"),
    }
}

/// `YYYY-MM-DD HH:mm:ss` as the GeneralizedTime `YYYYMMDDHHMMSSZ`.
pub fn generalized_time(date_time: &str) -> Result<String, SignError> {
    let compact = digits(date_time);
    let field = |from: usize, to: usize| -> Option<u32> {
        compact.get(from..to).and_then(|text| text.parse().ok())
    };
    let (year, month, day, hour, minute, second) = match compact.len() {
        14 => (
            field(0, 4),
            field(4, 6),
            field(6, 8),
            field(8, 10),
            field(10, 12),
            field(12, 14),
        ),
        // Seconds omitted.
        12 => (
            field(0, 4),
            field(4, 6),
            field(6, 8),
            field(8, 10),
            field(10, 12),
            Some(0),
        ),
        _ => return Err(bad_time(date_time)),
    };
    let missing = || bad_time(date_time);
    let (year, month, day, hour, minute, second) = (
        year.ok_or_else(missing)?,
        month.ok_or_else(missing)?,
        day.ok_or_else(missing)?,
        hour.ok_or_else(missing)?,
        minute.ok_or_else(missing)?,
        second.ok_or_else(missing)?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad_time(date_time)),
    };
    if !(1..=days_in_month).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return Err(bad_time(date_time));
    }
    Ok(format!(
        "{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}Z"
    ))
}

/// A certificate's time, `GeneralizedTime` or `UTCTime`, as a
/// `GeneralizedTime` (`YYYYMMDDHHMMSSZ`).
pub(crate) fn as_generalized_time(text: &str) -> Option<String> {
    let compact = digits(text);
    Some(match compact.len() {
        14 => format!("{compact}Z"),
        // UTCTime: 00..49 are 2000..2049, 50..99 are 1950..1999.
        12 => {
            let two: u32 = compact.get(..2)?.parse().ok()?;
            format!("{}{compact}Z", if two < 50 { 20 } else { 19 })
        }
        _ => return None,
    })
}
