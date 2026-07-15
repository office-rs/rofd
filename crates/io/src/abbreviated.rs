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
}
