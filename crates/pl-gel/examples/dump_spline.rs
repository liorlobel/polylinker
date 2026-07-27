//! Dump interpolated values so the gate can compare them with SciPy.
//!
//! stdin: one knot per line "x y", then a blank line, then one query x per line.
fn main() {
    let mut knots = Vec::new();
    let mut queries = Vec::new();
    let mut in_knots = true;
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).unwrap();
    for line in buf.lines() {
        let line = line.trim();
        if line.is_empty() {
            in_knots = false;
            continue;
        }
        if in_knots {
            let mut it = line.split_whitespace();
            knots.push((
                it.next().unwrap().parse::<f64>().unwrap(),
                it.next().unwrap().parse::<f64>().unwrap(),
            ));
        } else {
            queries.push(line.parse::<f64>().unwrap());
        }
    }
    let m = pl_gel::spline::Monotone::new(&knots).expect("valid knots");
    for q in queries {
        println!("{:.12e}", m.at(q));
    }
}
