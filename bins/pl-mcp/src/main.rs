//! An MCP server over stdio, so an assistant can ask about a plasmid.
//!
//! # What it will and will not do
//!
//! Every tool here is **read-only**. It reads files the caller names and
//! computes answers; nothing writes, converts, or edits. An assistant driving a
//! tool server is exactly the situation where an accidental overwrite is
//! unrecoverable, and there is no undo across a process boundary. Conversion
//! and editing stay in `pl`, where a person typed the command.
//!
//! # It says what it does not know
//!
//! The results carry the same caveats the CLI prints. Feature annotation
//! reports how many database records have actually been reviewed — currently
//! none — rather than returning names as facts, and the gel says it is a model
//! and not a measurement. An assistant will repeat whatever it is handed, so
//! anything hedged in the terminal has to be hedged here too, or the hedge is
//! lost exactly where it matters most.
//!
//! # No dependencies
//!
//! JSON-RPC 2.0 over stdio, with [`json`] doing the parsing. The correctness
//! crates take no dependencies and this is their front door.

mod json;

use std::io::{BufRead, Write};

use json::{arr, obj, s, Value};

const PROTOCOL: &str = "2024-11-05";

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match json::parse(&line) {
            Ok(req) => handle(&req),
            Err(e) => Some(error_for(
                &Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )),
        };
        // A notification has no id and gets no reply — answering one is a
        // protocol violation that some clients treat as fatal.
        if let Some(r) = response {
            let _ = writeln!(out, "{}", json::write(&r));
            let _ = out.flush();
        }
    }
}

fn handle(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let is_notification = req.get("id").is_none();

    let result = match method {
        "initialize" => Ok(obj(vec![
            ("protocolVersion", s(PROTOCOL)),
            ("capabilities", obj(vec![("tools", obj(vec![]))])),
            (
                "serverInfo",
                obj(vec![
                    ("name", s("polylinker")),
                    ("version", s(env!("CARGO_PKG_VERSION"))),
                ]),
            ),
        ])),
        "notifications/initialized" | "initialized" => return None,
        "tools/list" => Ok(obj(vec![("tools", arr(tool_list()))])),
        "tools/call" => call(req.get("params").unwrap_or(&Value::Null)),
        other => Err(format!("unknown method {other:?}")),
    };

    if is_notification {
        return None;
    }
    Some(match result {
        Ok(v) => obj(vec![("jsonrpc", s("2.0")), ("id", id), ("result", v)]),
        Err(e) => error_for(&id, -32601, &e),
    })
}

fn error_for(id: &Value, code: i64, message: &str) -> Value {
    obj(vec![
        ("jsonrpc", s("2.0")),
        ("id", id.clone()),
        (
            "error",
            obj(vec![
                ("code", Value::Number(code as f64)),
                ("message", s(message)),
            ]),
        ),
    ])
}

/// A tool's JSON Schema, in the shape MCP expects.
fn tool(name: &str, description: &str, props: Vec<(&str, &str, &str)>, required: &[&str]) -> Value {
    let mut p = std::collections::BTreeMap::new();
    for (k, ty, desc) in props {
        p.insert(
            k.to_string(),
            obj(vec![("type", s(ty)), ("description", s(desc))]),
        );
    }
    obj(vec![
        ("name", s(name)),
        ("description", s(description)),
        (
            "inputSchema",
            obj(vec![
                ("type", s("object")),
                ("properties", Value::Object(p)),
                ("required", arr(required.iter().map(|r| s(*r)).collect())),
            ]),
        ),
    ])
}

fn tool_list() -> Vec<Value> {
    vec![
        tool(
            "read_molecule",
            "Summarise a plasmid file: length, topology, GC, features, format. \
             Reads .dna, GenBank and FASTA.",
            vec![("path", "string", "Path to the file")],
            &["path"],
        ),
        tool(
            "digest",
            "Where restriction enzymes cut, and the fragments they produce. \
             Both strands, across the origin of a circular molecule.",
            vec![
                ("path", "string", "Path to the file"),
                (
                    "enzymes",
                    "string",
                    "Comma-separated enzyme names; omit for all",
                ),
            ],
            &["path"],
        ),
        tool(
            "melting_temperature",
            "Nearest-neighbour Tm for one or more oligos.",
            vec![("oligos", "string", "Comma-separated sequences")],
            &["oligos"],
        ),
        tool(
            "open_reading_frames",
            "ORFs in six frames, honouring the NCBI genetic code. Note that 13 of \
             the 27 codes do not treat TGA as a stop.",
            vec![
                ("path", "string", "Path to the file"),
                (
                    "table",
                    "number",
                    "NCBI transl_table number; default 11 (bacterial)",
                ),
                ("min_aa", "number", "Shortest ORF to report; default 30"),
            ],
            &["path"],
        ),
        tool(
            "checksum",
            "SEGUID v2 checksum — the same for the same molecule however it was \
             rotated or which strand was written first.",
            vec![("path", "string", "Path to the file")],
            &["path"],
        ),
        tool(
            "annotate",
            "Find known features. The shipped database is entirely unreviewed, so \
             this returns nothing unless include_proposed is set, and anything it \
             does return is a suggestion to check rather than an identification.",
            vec![
                ("path", "string", "Path to the file"),
                (
                    "include_proposed",
                    "boolean",
                    "Search records no human has signed off",
                ),
            ],
            &["path"],
        ),
        tool(
            "methods",
            "The methods paragraph for an operation, with its limits — generated \
             from the parameters the code actually uses.",
            vec![(
                "topic",
                "string",
                "tm, digest, gel, orfs, sanger, annotate, checksum, goldengate or primers",
            )],
            &["topic"],
        ),
    ]
}

fn text_result(text: String) -> Value {
    obj(vec![(
        "content",
        arr(vec![obj(vec![("type", s("text")), ("text", s(text))])]),
    )])
}

/// A tool that failed, reported as a *result* with `isError`, not as a
/// JSON-RPC error.
///
/// The distinction is in the protocol and it matters: a JSON-RPC error means
/// the call was malformed, and clients often surface it as a broken server. "No
/// such file" is a perfectly well-formed call with a bad argument, and the
/// model should see it and correct itself.
fn tool_error(text: String) -> Value {
    obj(vec![
        (
            "content",
            arr(vec![obj(vec![("type", s("text")), ("text", s(text))])]),
        ),
        ("isError", Value::Bool(true)),
    ])
}

/// How many cut positions and fragment lengths one digest line lists.
const SHOWN: usize = 8;

/// What to append to a list of `n` things when only [`SHOWN`] of them are there.
///
/// `which` says *which* of them, because the two lists are ordered differently:
/// cut positions come back ascending, so the survivors are the first eight,
/// while fragments come back longest first, so they are the eight largest.
/// Empty when nothing was dropped, so a complete answer reads as one.
fn elided(n: usize, which: &str) -> String {
    if n > SHOWN {
        format!(" ({which} {SHOWN} of {n} shown)")
    } else {
        String::new()
    }
}

fn call(params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("tools/call needs a name")?;
    let a = params.get("arguments").cloned().unwrap_or(Value::Null);
    let arg = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();

    let load = |path: &str| -> Result<pl_core::Molecule, String> {
        let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        pl_fileio::load_with_report(&data)
            .map(|(m, _, _)| m)
            .map_err(|e| format!("{path}: {e}"))
    };

    Ok(match name {
        "read_molecule" => match load(&arg("path")) {
            Err(e) => tool_error(e),
            Ok(m) => {
                let c = pl_core::Composition::of(&m.seq);
                text_result(format!(
                    "{} bp {}, GC {}, {} feature(s), {} primer(s)",
                    m.seq.len(),
                    m.topology.as_str(),
                    // `None` when the molecule holds no unambiguous bases —
                    // "0.0%" would be a claim about a sequence there is nothing
                    // to say about.
                    c.gc_percent()
                        .map(|g| format!("{g:.1}%"))
                        .unwrap_or_else(|| "unknown".into()),
                    m.features.len(),
                    m.primers.len()
                ))
            }
        },
        "digest" => match load(&arg("path")) {
            Err(e) => tool_error(e),
            Ok(m) => {
                let wanted = arg("enzymes");
                let names: Vec<&str> = wanted
                    .split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .collect();
                let mut lines = Vec::new();
                for d in pl_enzymes::digest_all(&m) {
                    if !names.is_empty()
                        && !names.iter().any(|n| n.eq_ignore_ascii_case(d.enzyme.name))
                    {
                        continue;
                    }
                    if d.positions.is_empty() {
                        continue;
                    }
                    let f = d.fragments(m.len(), m.topology);
                    // Both lists are capped, and a cap that does not say so is
                    // the whole failure. `fragments` comes back largest first,
                    // so the eight shown are the eight biggest bands and the
                    // rest are invisible: an assistant asked what the gel will
                    // look like reported eight bands for a digest that gives
                    // thirteen. The CLI appends ", ..." past six positions and
                    // heads its column "largest fragments"; nothing crossing
                    // this process boundary may be less honest than the
                    // terminal.
                    lines.push(format!(
                        "{}: {} cut(s) at {:?}{}, {} fragment(s) {:?}{}",
                        d.enzyme.name,
                        d.positions.len(),
                        &d.positions[..d.positions.len().min(SHOWN)],
                        elided(d.positions.len(), "first"),
                        f.len(),
                        &f[..f.len().min(SHOWN)],
                        elided(f.len(), "largest"),
                    ));
                }
                if lines.is_empty() {
                    lines.push("no cuts".into());
                }
                text_result(lines.join("\n"))
            }
        },
        "melting_temperature" => {
            let m = pl_thermo::Method::default();
            let mut lines = vec![format!("Method: {}", m.describe())];
            for o in arg("oligos")
                .split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
            {
                match pl_thermo::tm(o.as_bytes(), &m) {
                    Ok(t) => lines.push(format!("{o}  {:.1} C  ({} nt)", t.tm, o.len())),
                    Err(e) => lines.push(format!("{o}  cannot be computed: {e:?}")),
                }
            }
            text_result(lines.join("\n"))
        }
        "open_reading_frames" => match load(&arg("path")) {
            Err(e) => tool_error(e),
            Ok(m) => {
                // Checked *before* it is narrowed, and reported as the caller
                // wrote it.
                //
                // `as u8` on the way in truncated to the low byte, so a request
                // for table 267 silently became table 11 and -243 became 13 —
                // both real NCBI codes, so the guard below never fired and the
                // ORFs came back computed under a genetic code nobody asked
                // for. Table 300 did reach the error, and named 44.
                let id = a.get("table").and_then(Value::as_i64).unwrap_or(11);
                let Some(code) = u8::try_from(id).ok().and_then(pl_core::translate::table) else {
                    return Ok(tool_error(format!("no NCBI code {id}")));
                };
                // Likewise: `-1 as usize` is 18,446,744,073,709,551,615, which
                // no ORF can reach, and the reply was then byte-identical to a
                // molecule that genuinely has no ORF at the threshold asked
                // for.
                let want = a.get("min_aa").and_then(Value::as_i64).unwrap_or(30);
                let Ok(min_aa) = usize::try_from(want) else {
                    return Ok(tool_error(format!(
                        "min_aa must be zero or more, not {want}"
                    )));
                };
                let p = pl_core::orf::Params {
                    min_aa,
                    ..Default::default()
                };
                let orfs = pl_core::orf::find_orfs(&m.seq, code, m.topology.is_circular(), &p);
                let mut lines = vec![format!("table {id} — {}", code.name())];
                // The empty case is a result, not the absence of one. Without
                // this line the whole reply was the table header, which an
                // assistant reads as "this plasmid has no open reading frames"
                // — a claim about the molecule rather than about the threshold.
                // The CLI prints it; a hedge that does not survive the process
                // boundary is lost exactly where it matters most.
                if orfs.is_empty() {
                    lines.push(format!("no ORF of {min_aa} aa or more"));
                }
                for o in orfs.iter().take(40) {
                    lines.push(format!(
                        "{}..{} {} {} aa, starts {}{}",
                        o.start,
                        o.end,
                        if o.strand == pl_core::Strand::Reverse {
                            "-"
                        } else {
                            "+"
                        },
                        o.aa_len,
                        String::from_utf8_lossy(&o.start_codon),
                        if o.wrapped { " (crosses origin)" } else { "" }
                    ));
                }
                if orfs.len() > 40 {
                    lines.push(format!("... and {} more", orfs.len() - 40));
                }
                if !code.is_stop(b"TGA") {
                    lines.push(format!(
                        "note: table {id} reads TGA as an amino acid, not a stop"
                    ));
                }
                text_result(lines.join("\n"))
            }
        },
        "checksum" => match load(&arg("path")) {
            Err(e) => tool_error(e),
            Ok(m) => {
                let w = String::from_utf8_lossy(&m.seq).to_string();
                let r = if m.topology.is_circular() {
                    let c =
                        String::from_utf8_lossy(&pl_core::reverse_complement(&m.seq)).to_string();
                    pl_core::cdseguid(&w, &c).map(|x| format!("cdseguid: {x}"))
                } else {
                    pl_core::lsseguid(&w).map(|x| format!("lsseguid: {x}"))
                };
                match r {
                    Ok(x) => text_result(x),
                    // A checksum over a sequence with a character the algorithm
                    // does not define is not a checksum, and returning one
                    // anyway is how two different molecules come to look equal.
                    Err(e) => tool_error(format!("no checksum for this sequence: {e:?}")),
                }
            }
        },
        "annotate" => match load(&arg("path")) {
            Err(e) => tool_error(e),
            Ok(m) => {
                let (all, _) = pl_features::Db::builtin();
                let proposed = a
                    .get("include_proposed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let db = if proposed {
                    all.clone()
                } else {
                    all.reviewed()
                };
                if db.records.is_empty() {
                    // The caveat has to survive the process boundary. An
                    // assistant repeats what it is handed, so "nothing found"
                    // without the reason would be read as "this plasmid has no
                    // known features".
                    return Ok(text_result(format!(
                        "Nothing was searched: {} of {} database records have been \
                         reviewed by a named curator. The rest were assembled by \
                         machine from public sources and are not used by default. \
                         Set include_proposed to search them, and treat anything \
                         found as a suggestion to check against its cited accession.",
                        all.reviewed().records.len(),
                        all.records.len()
                    )));
                }
                let ann = pl_features::annotate::Annotator::new(
                    &db,
                    pl_features::annotate::Config::default(),
                );
                let found = ann.annotate(&m);
                let mut lines: Vec<String> = found
                    .iter()
                    .map(|f| {
                        format!(
                            "{}..{} {} {} — {:.1}% identity, {:.0}% coverage{}",
                            f.start,
                            f.end,
                            if f.strand == pl_core::Strand::Reverse {
                                "-"
                            } else {
                                "+"
                            },
                            db.records[f.record].name,
                            f.identity * 100.0,
                            f.coverage * 100.0,
                            if f.is_fragment { ", fragment" } else { "" }
                        )
                    })
                    .collect();
                if lines.is_empty() {
                    lines.push("nothing found".into());
                    // The same caveat as the empty-database branch above, for the
                    // same reason, and it has to be in both places: that branch
                    // only fires when NOTHING is reviewed, so signing the table off
                    // on 2026-07-28 silenced it and left a bare "nothing found"
                    // crossing the boundary. An assistant repeats what it is
                    // handed, and "nothing found" out of an 84-record database gets
                    // relayed as "this plasmid has no known features" — a claim
                    // about the molecule made out of a fact about the database.
                    // The count is the bound on what the answer can mean.
                    lines.push(format!(
                        "This means none of the {} curated record(s) searched were \
                         found — not that the molecule has no known features. The \
                         database is deliberately small and is not comprehensive.",
                        db.records.len()
                    ));
                }
                if proposed {
                    lines.push(
                        "These come from records no human has reviewed. Check each \
                         against its source before treating it as an identification."
                            .into(),
                    );
                }
                text_result(lines.join("\n"))
            }
        },
        "methods" => match pl_doc::topic(&arg("topic")) {
            Some(t) => text_result(pl_doc::methods(t)),
            None => tool_error(format!(
                "unknown topic; try one of {}",
                pl_doc::TOPICS
                    .iter()
                    .map(|t| t.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        },
        other => tool_error(format!("unknown tool {other:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> Option<Value> {
        handle(&json::parse(text).expect(text))
    }

    /// A file on disk holding `text`, for the tools that take a path.
    ///
    /// Named per process so two test binaries running at once cannot read each
    /// other's fixture half-written.
    fn fixture(name: &str, text: &str) -> String {
        let dir = std::env::temp_dir().join(format!("pl-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp directory");
        let p = dir.join(name);
        std::fs::write(&p, text).expect("a fixture");
        p.display().to_string()
    }

    /// Call one tool and return the text it replied with, error or not.
    fn call_tool(name: &str, args: Vec<(&str, Value)>) -> String {
        let call = json::write(&obj(vec![
            ("jsonrpc", s("2.0")),
            ("id", Value::Number(99.0)),
            ("method", s("tools/call")),
            (
                "params",
                obj(vec![("name", s(name)), ("arguments", obj(args))]),
            ),
        ]));
        let r = req(&call).expect("a reply");
        assert!(r.get("error").is_none(), "not a protocol error: {r:?}");
        r.get("result")
            .unwrap()
            .get("content")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn initialize_answers_with_the_protocol_version_and_a_name() {
        let r = req(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).expect("a reply");
        assert_eq!(
            r.get("result")
                .unwrap()
                .get("protocolVersion")
                .unwrap()
                .as_str(),
            Some(PROTOCOL)
        );
        assert_eq!(r.get("id").unwrap().as_i64(), Some(1));
        assert_eq!(r.get("jsonrpc").unwrap().as_str(), Some("2.0"));
    }

    #[test]
    fn a_notification_gets_no_reply() {
        // Answering one is a protocol violation, and some clients treat an
        // unexpected response as a broken server.
        assert!(req(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(req(r#"{"jsonrpc":"2.0","method":"tools/list"}"#).is_none());
        // The same call *with* an id does get one.
        assert!(req(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).is_some());
    }

    #[test]
    fn every_tool_declares_a_schema_a_client_can_use() {
        let r = req(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        let tools = r
            .get("result")
            .unwrap()
            .get("tools")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(tools.len() >= 7);
        for t in tools {
            let name = t.get("name").unwrap().as_str().unwrap();
            assert!(!name.is_empty());
            assert!(
                t.get("description").unwrap().as_str().unwrap().len() > 20,
                "{name} has no useful description"
            );
            let schema = t.get("inputSchema").unwrap();
            assert_eq!(schema.get("type").unwrap().as_str(), Some("object"));
            let required = schema.get("required").unwrap().as_array().unwrap();
            let props = schema.get("properties").unwrap();
            for r in required {
                let k = r.as_str().unwrap();
                assert!(
                    props.get(k).is_some(),
                    "{name} requires {k} and does not declare it"
                );
            }
        }
    }

    #[test]
    fn no_tool_can_write_anything() {
        // Every tool here reads. An assistant driving a tool server is exactly
        // where an accidental overwrite is unrecoverable, and there is no undo
        // across a process boundary.
        let r = req(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        let tools = r
            .get("result")
            .unwrap()
            .get("tools")
            .unwrap()
            .as_array()
            .unwrap();
        for t in tools {
            let name = t.get("name").unwrap().as_str().unwrap();
            for forbidden in ["write", "save", "convert", "edit", "delete", "export"] {
                assert!(
                    !name.contains(forbidden),
                    "{name} sounds like it mutates something"
                );
            }
        }
    }

    #[test]
    fn a_bad_argument_is_a_tool_error_and_not_a_protocol_error() {
        // A JSON-RPC error means the *call* was malformed and clients often
        // surface it as a broken server. "No such file" is a well-formed call
        // with a bad argument, and the model should see it and correct itself.
        let r = req(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call",
                "params":{"name":"read_molecule","arguments":{"path":"nope.gb"}}}"#)
        .unwrap();
        assert!(r.get("error").is_none(), "not a protocol error: {r:?}");
        let res = r.get("result").unwrap();
        assert_eq!(res.get("isError").unwrap().as_bool(), Some(true));
        let text = res.get("content").unwrap().as_array().unwrap()[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(text.contains("nope.gb"), "{text}");
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error() {
        let r = req(r#"{"jsonrpc":"2.0","id":4,"method":"nope"}"#).unwrap();
        assert_eq!(
            r.get("error").unwrap().get("code").unwrap().as_i64(),
            Some(-32601)
        );
    }

    #[test]
    fn annotate_carries_its_caveat_across_the_process_boundary() {
        // The whole point. An assistant repeats what it is handed, so "nothing
        // found" without the reason would be read as "this plasmid has no known
        // features" — which is a claim the database cannot support.
        // Paths in a test are relative to the crate, not the workspace.
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/library-fixture/a.gb");
        assert!(fixture.exists(), "{}", fixture.display());
        let call = json::write(&obj(vec![
            ("jsonrpc", s("2.0")),
            ("id", Value::Number(5.0)),
            ("method", s("tools/call")),
            (
                "params",
                obj(vec![
                    ("name", s("annotate")),
                    (
                        "arguments",
                        obj(vec![("path", s(fixture.display().to_string()))]),
                    ),
                ]),
            ),
        ]));
        let r = req(&call).unwrap();
        let text = r
            .get("result")
            .unwrap()
            .get("content")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap();
        // Until 2026-07-28 this asserted the *other* caveat — "Nothing was
        // searched: 0 of N reviewed" — because the shipped table was entirely
        // unsigned. Signing it off silenced that branch and left this fixture
        // returning a bare "nothing found", which is exactly the sentence the
        // test exists to prevent an assistant from relaying. The caveat moved
        // rather than disappeared, so the assertion moves with it.
        assert!(text.contains("nothing found"), "{text}");
        assert!(
            text.contains("not that the molecule has no known features"),
            "an empty result crossed the boundary without its bound: {text}"
        );
        assert!(text.contains("curated record(s) searched"), "{text}");
    }

    #[test]
    fn the_methods_tool_returns_text_with_limits_in_it() {
        let r = req(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call",
                "params":{"name":"methods","arguments":{"topic":"goldengate"}}}"#)
        .unwrap();
        let text = r
            .get("result")
            .unwrap()
            .get("content")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(text.contains("Limits:"), "{text}");
        assert!(text.contains("no fidelity percentage"), "{text}");
    }

    #[test]
    fn a_tm_request_reports_the_method_alongside_the_number() {
        // A temperature with no model behind it is not a result somebody can
        // put in a paper.
        let r = req(
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call",
                "params":{"name":"melting_temperature","arguments":{"oligos":"GTAAAACGACGGCCAGT, ACGT"}}}"#,
        )
        .unwrap();
        let text = r
            .get("result")
            .unwrap()
            .get("content")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(text.starts_with("Method:"), "{text}");
        assert!(text.contains("GTAAAACGACGGCCAGT"), "{text}");
    }

    /// A 1,300 bp record with a PvuII site every hundred bases: 13 cuts, and
    /// 14 fragments once linear.
    fn thirteen_cutter() -> String {
        let unit = format!("CAGCTG{}", "A".repeat(94));
        fixture("many-cuts.fa", &format!(">many\n{}\n", unit.repeat(13)))
    }

    #[test]
    fn a_digest_that_lists_only_some_of_its_fragments_says_so() {
        // Both lists stop at eight. The cut count made the position truncation
        // inferable; the fragment count was never stated at all, so an
        // assistant asked what the gel would look like reported eight bands
        // for a digest that gives fourteen — a partial pattern that reads as
        // complete.
        let text = call_tool(
            "digest",
            vec![("path", s(thirteen_cutter())), ("enzymes", s("PvuII"))],
        );
        assert!(text.contains("13 cut(s)"), "{text}");
        assert!(text.contains("first 8 of 13 shown"), "{text}");
        assert!(text.contains("14 fragment(s)"), "{text}");
        assert!(text.contains("largest 8 of 14 shown"), "{text}");
    }

    #[test]
    fn a_digest_short_enough_to_show_whole_claims_no_truncation() {
        // The control. A note on a complete list would be its own lie, and an
        // assistant would hedge an answer that needs no hedging.
        let path = fixture("one-cut.fa", ">one\nAAAACAGCTGAAAA\n");
        let text = call_tool("digest", vec![("path", s(path)), ("enzymes", s("PvuII"))]);
        assert!(text.contains("1 cut(s)"), "{text}");
        assert!(text.contains("2 fragment(s)"), "{text}");
        assert!(!text.contains("shown)"), "{text}");
    }

    #[test]
    fn a_genetic_code_outside_a_byte_is_refused_rather_than_wrapped() {
        // `as u8` truncated to the low byte, so 267 became 11 and -243 became
        // 13 — both real NCBI codes. The guard never fired, and the ORFs came
        // back computed under a genetic code the caller did not ask for. 300
        // did reach the error and named 44, a number nobody sent.
        let path = fixture("orf.fa", ">x\nATGAAACCCGGGTAA\n");
        for n in [267.0, -243.0, 300.0, 1e19] {
            let text = call_tool(
                "open_reading_frames",
                vec![("path", s(path.clone())), ("table", Value::Number(n))],
            );
            assert!(
                text.starts_with("no NCBI code") && text.contains(&format!("{}", n as i64)),
                "table {n} gave {text}"
            );
        }
    }

    #[test]
    fn a_real_genetic_code_is_still_accepted_and_named() {
        // The control for the check above: narrowing was wrong, refusing
        // everything would be worse.
        let path = fixture("orf.fa", ">x\nATGAAACCCGGGTAA\n");
        for n in [1.0, 2.0, 11.0, 33.0] {
            let text = call_tool(
                "open_reading_frames",
                vec![("path", s(path.clone())), ("table", Value::Number(n))],
            );
            assert!(
                text.starts_with(&format!("table {} — ", n as i64)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_min_aa_that_is_not_a_length_is_refused_rather_than_finding_nothing() {
        // `-1 as usize` is 18,446,744,073,709,551,615, which no ORF can reach.
        let path = fixture("orf.fa", ">x\nATGAAACCCGGGTAA\n");
        let text = call_tool(
            "open_reading_frames",
            vec![("path", s(path)), ("min_aa", Value::Number(-1.0))],
        );
        assert!(
            text.contains("min_aa must be zero or more, not -1"),
            "{text}"
        );
    }

    #[test]
    fn an_orf_search_that_finds_nothing_says_so_rather_than_stopping_at_the_header() {
        // The reply used to be the table header alone — byte-identical to a
        // molecule that genuinely has no ORF, with no echo of the threshold
        // actually used. An assistant reads that as "this plasmid has no open
        // reading frames", which is a claim about the molecule.
        let path = fixture("orf.fa", ">x\nATGAAACCCGGGTAA\n");
        let text = call_tool(
            "open_reading_frames",
            vec![("path", s(path)), ("min_aa", Value::Number(1000.0))],
        );
        assert!(text.contains("no ORF of 1000 aa or more"), "{text}");
    }
}
