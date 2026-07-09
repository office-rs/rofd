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
  <ofd:Common><ofd:PageArea><ofd:PhysicalBox x="0" y="0" w="210" h="297"/></ofd:PageArea></ofd:Common>
  <ofd:Pages>
    <ofd:Page ID="P0" BaseLoc="Pages/Page_0/Page.xml"/>
  </ofd:Pages>
</ofd:Document>"#;

const PAGE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox x="0" y="0" w="210" h="297"/></ofd:Area>
  <ofd:Content>
    <ofd:Layer Type="Body">
      <ofd:TextObject ID="t1" Boundary="10 10 100 20" Font="F1" Size="12">
        <ofd:FillColor Color="0 0 0"/>
        <ofd:TextCode X="0" Y="14" DeltaX="0">Hello</ofd:TextCode>
      </ofd:TextObject>
      <ofd:PathObject ID="p1" Boundary="10 40 100 10" LineWidth="1" Stroke="true" Fill="false">
        <ofd:AbbreviatedData>M 0 0 L 100 0 L 100 10 C 50 10 0 5 0 0 Z</ofd:AbbreviatedData>
        <ofd:StrokeColor Color="255 0 0"/>
      </ofd:PathObject>
    </ofd:Layer>
  </ofd:Content>
  <ofd:Annotation><ofd:File Loc="Page_0/Annotation.xml"/></ofd:Annotation>
</ofd:Page>"#;

const ANNOTATION_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Annotation ID="a1" Type="Highlight">
    <ofd:Appearance><ofd:Page><ofd:Area><ofd:PhysicalBox x="0" y="0" w="210" h="297"/></ofd:Area></ofd:Page></ofd:Appearance>
    <ofd:Color Color="255 255 0"/>
    <ofd:Creator>tester</ofd:Creator>
    <ofd:CreationDate>2026-07-08T00:00:00</ofd:CreationDate>
    <ofd:LastModDate>2026-07-08T00:00:00</ofd:LastModDate>
  </ofd:Annotation>
</ofd:Annotations>"#;

const FONT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Font ID="F1" FontName="NotoSans"/>
</ofd:Res>"#;

/// Build a minimal but valid-shaped .ofd ZIP in memory.
pub fn build_minimal_ofd() -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("OFD.xml", OFD_XML),
        ("Doc_0/Document.xml", DOCUMENT_XML),
        ("Doc_0/Pages/Page_0/Page.xml", PAGE_XML),
        ("Doc_0/Pages/Page_0/Annotation.xml", ANNOTATION_XML),
        ("Doc_0/Res/Font.xml", FONT_XML),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
