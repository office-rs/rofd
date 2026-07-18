use std::io::Write;
use zip::write::ZipWriter;

const OFD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:DocBody>
    <ofd:DocInfo>
      <ofd:DocID>doc-001</ofd:DocID>
      <ofd:Title>fixture</ofd:Title>
      <ofd:Author>tester</ofd:Author>
    </ofd:DocInfo>
    <ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>
  </ofd:DocBody>
</ofd:OFD>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>
  <ofd:MaxUnitID>101</ofd:MaxUnitID></ofd:CommonData>
  <ofd:Pages>
    <ofd:Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/>
  </ofd:Pages>
  <ofd:Annotations>Annots/Annotations.xml</ofd:Annotations>
</ofd:Document>"#;

const PAGE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:TextObject ID="t1" Boundary="10 10 100 20" Font="F1" Size="12">
        <ofd:FillColor Value="0 0 0"/>
        <ofd:TextCode X="0" Y="14" DeltaX="0">Hello</ofd:TextCode>
      </ofd:TextObject>
      <ofd:PathObject ID="p1" Boundary="10 40 100 10" LineWidth="1" Stroke="true" Fill="false">
        <ofd:AbbreviatedData>M 0 0 L 100 0 L 100 10 C 50 10 0 5 0 0 Z</ofd:AbbreviatedData>
        <ofd:StrokeColor Value="255 0 0"/>
      </ofd:PathObject>
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>"#;

/// GB/T 33190 §15.1 annotation entry file (Annotations.xml): references the
/// per-page PageAnnot file by PageID + FileLoc.
const ANNOTATION_ENTRY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Page PageID="1"><ofd:FileLoc>Page_0/Annotation.xml</ofd:FileLoc></ofd:Page>
</ofd:Annotations>"#;

/// GB/T 33190 §15.2 per-page PageAnnot: <Annot Type Subtype ID Creator
/// LastModDate><Appearance Boundary><PathObject>. Standard structure.
const ANNOTATION_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annot Type="Highlight" Subtype="Highlight" ID="100" Creator="tester" LastModDate="2026-07-08">
    <ofd:Appearance Boundary="10 10 100 20">
      <ofd:PathObject ID="101" Boundary="10 10 100 20" LineWidth="1">
        <ofd:StrokeColor Value="255 255 0"/>
      </ofd:PathObject>
    </ofd:Appearance>
  </ofd:Annot>
</ofd:PageAnnot>"#;

const FONT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Font ID="F1" FontName="NotoSans"/>
</ofd:Res>"#;

#[allow(dead_code)]
const FONT_XML_WITH_FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Font ID="F1" FontName="NotoSans"><ofd:FontFile>Font_1.ttf</ofd:FontFile></ofd:Font>
</ofd:Res>"#;

/// Page with a PathObject + ImageObject that reference a DrawParam / MultiMedia
/// (no inline colors) - exercises the DocumentRes.xml resource resolution.
const PAGE_WITH_DRAWPARAM_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:PathObject ID="p1" Boundary="10 40 100 10" Stroke="true" DrawParam="5">
        <ofd:AbbreviatedData>M 0 0 L 100 0</ofd:AbbreviatedData>
      </ofd:PathObject>
      <ofd:ImageObject ID="i1" ResourceID="9" Boundary="10 10 50 20"/>
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>"#;

/// Page XML containing an unknown element `<ofd:CustomWidget/>` inside the
/// body layer. Exercises the SkippedObject warning path (AGENTS.md §4.6:
/// unknown page elements are skipped + warning, not fatal).
const PAGE_WITH_UNKNOWN_ELEMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:TextObject ID="t1" Boundary="10 10 100 20" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="14" DeltaX="0">Hello</ofd:TextCode>
      </ofd:TextObject>
      <ofd:CustomWidget ID="cw1" Boundary="10 50 100 30"/>
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>"#;

/// Page XML containing a `<ofd:Template>` element (GB/T 33190 §7.5). v1 does
/// not expand templates; parse_page captures the template ref and parse_ofd
/// emits a MissingFeature warning.
const PAGE_WITH_TEMPLATE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>
  <ofd:Template>
    <ofd:Page ID="tmpl0"/>
  </ofd:Template>
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:TextObject ID="t1" Boundary="10 10 100 20" Font="F1" Size="12">
        <ofd:TextCode X="0" Y="14" DeltaX="0">Hello</ofd:TextCode>
      </ofd:TextObject>
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>"#;

/// Page XML with an ImageObject referencing ResourceID="9" (resolved via
/// DocumentRes.xml). Used with DOCUMENT_RES_MISSING_IMAGE_XML to test the
/// ResourceNotFound warning for missing image files.
const PAGE_WITH_IMAGE_OBJECT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:ImageObject ID="i1" ResourceID="9" Boundary="10 10 50 20"/>
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>"#;

/// DocumentRes.xml with a MultiMedia referencing an image file that is NOT
/// present in the ZIP. Exercises the ResourceNotFound warning path.
const DOCUMENT_RES_MISSING_IMAGE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016" BaseLoc="Res">
  <ofd:MultiMedias><ofd:MultiMedia Type="Image" Format="PNG" ID="9"><ofd:MediaFile>missing.png</ofd:MediaFile></ofd:MultiMedia></ofd:MultiMedias>
</ofd:Res>"#;

/// Font.xml referencing FontFile="missing.ttf" (file intentionally absent from
/// the ZIP). Exercises the FontSubstituted warning path.
const FONT_XML_MISSING_FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Font ID="F1" FontName="NotoSans"><ofd:FontFile>missing.ttf</ofd:FontFile></ofd:Font>
</ofd:Res>"#;

/// Malformed XML (bare `<` in text content) used for the bad-XML error test.
/// quick-xml rejects this as a syntax error, surfacing as `OfdError::Xml`.
const MALFORMED_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:CommonData><ofd:MaxUnitID>1 < 2</ofd:MaxUnitID></ofd:CommonData>
</ofd:Document>"#;

const DOCUMENT_RES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016" BaseLoc="Res">
  <ofd:DrawParams><ofd:DrawParam LineWidth="2.0" ID="5"><ofd:FillColor Value="0 0 0"/><ofd:StrokeColor Value="255 0 0"/></ofd:DrawParam></ofd:DrawParams>
  <ofd:MultiMedias><ofd:MultiMedia Type="Image" Format="PNG" ID="9"><ofd:MediaFile>img.png</ofd:MediaFile></ofd:MultiMedia></ofd:MultiMedias>
</ofd:Res>"#;

/// Build a minimal but valid-shaped .ofd ZIP in memory (GB/T 33190 standard
/// structure: Doc_0/Document.xml with Annotations loc, Doc_0/Annots/
/// Annotations.xml entry, Doc_0/Annots/Page_0/Annotation.xml per-page).
#[allow(dead_code)]
pub fn build_minimal_ofd() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE_XML),
        ("Doc_0/Annots/Annotations.xml", ANNOTATION_ENTRY_XML),
        ("Doc_0/Annots/Page_0/Annotation.xml", ANNOTATION_XML),
        ("Doc_0/Res/Font.xml", FONT_XML),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Like build_minimal_ofd but Font.xml references FontFile="Font_1.ttf" with dummy bytes.
#[allow(dead_code)]
pub fn build_minimal_ofd_with_font() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE_XML),
        ("Doc_0/Annots/Annotations.xml", ANNOTATION_ENTRY_XML),
        ("Doc_0/Annots/Page_0/Annotation.xml", ANNOTATION_XML),
        ("Doc_0/Res/Font.xml", FONT_XML_WITH_FILE),
        ("Doc_0/Res/Font_1.ttf", ""), // dummy font bytes (real font not needed for io parse test)
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// OFD whose page objects reference a DrawParam + MultiMedia defined in
/// DocumentRes.xml (no inline colors). Exercises resource resolution.
#[allow(dead_code)]
pub fn build_minimal_ofd_with_drawparam() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE_WITH_DRAWPARAM_XML),
        ("Doc_0/DocumentRes.xml", DOCUMENT_RES_XML),
        ("Doc_0/Res/img.png", ""), // dummy image bytes (parse test only)
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Build a valid ZIP whose OFD.xml references a Document.xml containing
/// malformed XML (unclosed tags). Used to test the `OfdError::Xml` path.
#[allow(dead_code)]
pub fn build_ofd_with_malformed_xml() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [("OFD.xml", OFD_XML), ("Doc_0/Document.xml", MALFORMED_XML)] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Build an OFD whose page contains an unknown element `<ofd:CustomWidget/>`.
/// Used to test the `SkippedObject` warning emission for unknown page elements.
#[allow(dead_code)]
pub fn build_ofd_with_unknown_page_element() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            PAGE_WITH_UNKNOWN_ELEMENT_XML,
        ),
        ("Doc_0/Res/Font.xml", FONT_XML),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Build an OFD whose page contains a `<ofd:Template>` element. v1 does not
/// expand templates; parse_ofd emits a `MissingFeature` warning.
#[allow(dead_code)]
pub fn build_ofd_with_template() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE_WITH_TEMPLATE_XML),
        ("Doc_0/Res/Font.xml", FONT_XML),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Build an OFD whose Font.xml references `FontFile="missing.ttf"` but the
/// font file is absent from the ZIP. parse_ofd emits `FontSubstituted`.
#[allow(dead_code)]
pub fn build_ofd_with_missing_font_file() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE_XML),
        ("Doc_0/Res/Font.xml", FONT_XML_MISSING_FILE),
        // missing.ttf intentionally absent
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Build an OFD whose DocumentRes.xml references an image file `missing.png`
/// that is absent from the ZIP. parse_ofd emits `ResourceNotFound`.
#[allow(dead_code)]
pub fn build_ofd_with_missing_image() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE_WITH_IMAGE_OBJECT_XML),
        ("Doc_0/DocumentRes.xml", DOCUMENT_RES_MISSING_IMAGE_XML),
        // missing.png intentionally absent
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
