use rofd_io::zip_util;

mod fixtures;

#[test]
fn read_all_entries_lists_fixture_parts() {
    let bytes = fixtures::build_minimal_ofd();
    let entries = zip_util::read_all_entries(&bytes).unwrap();
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"OFD.xml"));
    assert!(names.contains(&"Doc_0/Document.xml"));
    assert!(names.contains(&"Doc_0/Pages/Page_0/Page.xml"));
    assert!(names.contains(&"Doc_0/Pages/Page_0/Annotation.xml"));
}

#[test]
fn write_zip_round_trips_entries() {
    let bytes = fixtures::build_minimal_ofd();
    let entries = zip_util::read_all_entries(&bytes).unwrap();
    let rebuilt = zip_util::write_zip(&entries).unwrap();
    let again = zip_util::read_all_entries(&rebuilt).unwrap();
    assert_eq!(entries.len(), again.len());
}
