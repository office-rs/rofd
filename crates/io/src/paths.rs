//! Package paths: how the files of an OFD package locate one another
//! (GB/T 33190-2016, table 2, `ST_Loc`).
//!
//! A loc is package-absolute when it starts with `/`; anything else is
//! relative to the directory of the file that holds it. `.` is that
//! directory and `..` its parent. A package path as this crate holds it (an
//! entry name, a field of `PackageHandle`) has no leading slash and is fully
//! resolved.

/// `loc`, as written in a file that sits under `base_dir`, as a resolved
/// package path.
pub fn resolve_loc(loc: &str, base_dir: &str) -> String {
    let loc = loc.trim();
    match loc.strip_prefix('/') {
        Some(absolute) => normalize(absolute),
        None if base_dir.is_empty() => normalize(loc),
        None => normalize(&format!("{base_dir}/{loc}")),
    }
}

/// `path` as the loc that names it from a file under `dir`: plain when
/// `path` lies below `dir`, through `..` otherwise.
pub fn relative_to(path: &str, dir: &str) -> String {
    let base: Vec<&str> = split(dir);
    let target: Vec<&str> = split(path);
    let shared = base
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut segments: Vec<&str> = base[shared..].iter().map(|_| "..").collect();
    segments.extend_from_slice(&target[shared..]);
    if segments.is_empty() {
        return ".".to_owned();
    }
    segments.join("/")
}

fn split(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect()
}

/// `.` and `..` resolved; `..` at the package root stays at the root.
fn normalize(path: &str) -> String {
    let mut resolved: Vec<&str> = Vec::new();
    for segment in split(path) {
        if segment == ".." {
            resolved.pop();
        } else {
            resolved.push(segment);
        }
    }
    resolved.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loc_resolves_against_its_file_dot_segments_and_all() {
        assert_eq!(
            resolve_loc("Annots/Annotations.xml", "Doc_0"),
            "Doc_0/Annots/Annotations.xml"
        );
        assert_eq!(
            resolve_loc("/Doc_0/Document.xml", "Doc_0/Signs"),
            "Doc_0/Document.xml"
        );
        assert_eq!(
            resolve_loc("Sign_0/Signature.xml", ""),
            "Sign_0/Signature.xml"
        );
        assert_eq!(
            resolve_loc(" Sign_0/Signature.xml ", "Doc_0/Signs"),
            "Doc_0/Signs/Sign_0/Signature.xml"
        );
        // GB/T 33190 table 2 lists `..` and `.` as part of a loc.
        assert_eq!(
            resolve_loc("../Pages/Page_0/Annotation.xml", "Doc_0/Annots"),
            "Doc_0/Pages/Page_0/Annotation.xml"
        );
        assert_eq!(
            resolve_loc("./Res/Font.xml", "Doc_0/Pages/Page_0"),
            "Doc_0/Pages/Page_0/Res/Font.xml"
        );
        assert_eq!(
            resolve_loc("/Doc_0/Pages/../Document.xml", ""),
            "Doc_0/Document.xml"
        );
        // Above the package root there is nothing: the loc stays at the root.
        assert_eq!(resolve_loc("../../OFD.xml", "Doc_0"), "OFD.xml");
        assert_eq!(resolve_loc("..", "Doc_0"), "");
    }

    #[test]
    fn a_path_is_named_relative_to_a_directory() {
        assert_eq!(
            relative_to("Doc_0/Annots/Annotations.xml", "Doc_0"),
            "Annots/Annotations.xml"
        );
        assert_eq!(
            relative_to("Doc_0/Annots/P_7.xml", "Doc_0/Annots"),
            "P_7.xml"
        );
        assert_eq!(relative_to("Annotations.xml", ""), "Annotations.xml");
        // Not below the directory: up to the shared one, then down.
        assert_eq!(
            relative_to("Doc_0/Pages/Page_0/Annotation.xml", "Doc_0/Annots"),
            "../Pages/Page_0/Annotation.xml"
        );
        assert_eq!(
            relative_to("Doc_0/Document.xml", "Doc_0/Signs"),
            "../Document.xml"
        );
        assert_eq!(
            relative_to("Doc_1/Document.xml", "Doc_0/Annots"),
            "../../Doc_1/Document.xml"
        );
        // A shared prefix is whole segments, not a string prefix.
        assert_eq!(
            relative_to("Doc_0/Annots/P_7.xml", "Doc_0/Annot"),
            "../Annots/P_7.xml"
        );
        assert_eq!(relative_to("Doc_0/Annots", "Doc_0/Annots"), ".");
    }

    #[test]
    fn what_one_direction_writes_the_other_reads() {
        for (path, dir) in [
            ("Doc_0/Pages/Page_0/Annotation.xml", "Doc_0/Annots"),
            ("Doc_0/Annots/P_7.xml", "Doc_0/Annots"),
            ("Doc_0/Annots/Annotations.xml", "Doc_0"),
            ("Annotations.xml", ""),
            ("Doc_1/Pages/Page_9/Annotation.xml", ""),
        ] {
            assert_eq!(resolve_loc(&relative_to(path, dir), dir), path);
        }
    }
}
