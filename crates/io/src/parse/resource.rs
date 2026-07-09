use quick_xml::events::Event;
use quick_xml::Reader;

use rofd_dom::{FontId, FontRef, Resources};

use crate::error::OfdError;
use crate::parse::attr;

pub fn parse_font_res(font_xml: &str, resources: &mut Resources) -> Result<(), OfdError> {
    let mut reader = Reader::from_str(font_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().local_name().as_ref() == b"Font" => {
                let id = FontId::new(attr(&e, "ID").unwrap_or_default());
                let family = attr(&e, "FontName");
                resources.fonts.insert(id.clone(), FontRef { id, family_name: family });
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(OfdError::Xml { entry: "Font.xml".into(), loc: String::new(), source: e }),
            _ => {}
        }
    }
    Ok(())
}
