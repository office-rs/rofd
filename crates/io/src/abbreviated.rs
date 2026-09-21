use rofd_dom::{PathCommand, PathData};

/// Parse OFD AbbreviatedData, e.g. "M 0 0 L 100 0 C 1 2 3 4 5 6 Z".
pub fn parse_abbreviated(s: &str) -> PathData {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut cmds = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let op = toks[i];
        i += 1;
        let f = |idx: usize| -> (f64, usize) {
            let v = toks.get(idx).and_then(|t| t.parse().ok()).unwrap_or(0.0);
            (v, idx + 1)
        };
        match op {
            "M" => {
                let (x, n) = f(i);
                let (y, n) = f(n);
                i = n;
                cmds.push(PathCommand::M(x, y));
            }
            "L" => {
                let (x, n) = f(i);
                let (y, n) = f(n);
                i = n;
                cmds.push(PathCommand::L(x, y));
            }
            "C" => {
                // "C" is dual-purpose on the wire: followed by numbers it is a
                // 6-param cubic Bezier; bare (next token missing or another
                // operator) it is a close, the dialect reference authoring
                // tools use to terminate rect/polygon paths.
                let next_is_number = toks
                    .get(i)
                    .map(|t| t.parse::<f64>().is_ok())
                    .unwrap_or(false);
                if !next_is_number {
                    cmds.push(PathCommand::Z);
                    continue;
                }
                let (a, n) = f(i);
                let (b, n) = f(n);
                let (c, n) = f(n);
                let (d, n) = f(n);
                let (e, n) = f(n);
                let (g, n) = f(n);
                i = n;
                cmds.push(PathCommand::C(a, b, c, d, e, g));
            }
            "Q" => {
                let (a, n) = f(i);
                let (b, n) = f(n);
                let (c, n) = f(n);
                let (d, n) = f(n);
                i = n;
                cmds.push(PathCommand::Q(a, b, c, d));
            }
            "A" => {
                // GB/T 33190 A arc has 7 params (rx ry rot large-arc-flag
                // sweep-flag x y). PathCommand::A carries 6 (rx ry rot sweep x
                // y), so skip the 4th (large-arc-flag) when parsing.
                let (rx, n) = f(i);
                let (ry, n) = f(n);
                let (rot, n) = f(n);
                let (_large_arc, n) = f(n); // skipped: quarter arcs are small-arc
                let (sweep, n) = f(n);
                let (x, n) = f(n);
                let (y, n) = f(n);
                i = n;
                cmds.push(PathCommand::A(rx, ry, rot, sweep, x, y));
            }
            "Z" | "S" => {
                cmds.push(PathCommand::Z);
            }
            _ => { /* unknown token: skip */ }
        }
    }
    PathData { commands: cmds }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_m_l_z() {
        let pd = parse_abbreviated("M 0 0 L 100 0 L 100 10 Z");
        assert_eq!(pd.commands.len(), 4);
    }

    #[test]
    fn bare_trailing_c_closes_the_path() {
        // Reference authoring tools close paths with a parameterless "C";
        // a numeric-followed "C" is a 6-param cubic. The close form must not
        // be consumed as a garbage cubic to (0, 0).
        let pd = parse_abbreviated("M 0 0 L 10 0 L 10 10 C");
        assert_eq!(pd.commands.len(), 4);
        assert!(matches!(pd.commands[3], PathCommand::Z));
    }

    #[test]
    fn numeric_c_still_parses_as_cubic() {
        let pd = parse_abbreviated("M 0 0 C 1 2 3 4 5 6");
        assert_eq!(pd.commands.len(), 2);
        assert!(matches!(pd.commands[1], PathCommand::C(..)));
    }

    #[test]
    fn bare_c_before_next_operator_closes() {
        // "C" directly followed by another operator (not a number) is a close.
        let pd = parse_abbreviated("M 0 0 L 10 0 C M 5 5 L 6 6");
        assert_eq!(pd.commands.len(), 5);
        assert!(matches!(pd.commands[2], PathCommand::Z));
    }
}
