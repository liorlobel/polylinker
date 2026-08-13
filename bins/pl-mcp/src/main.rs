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
//! reports how many of the records it searched a named curator has signed off,
//! and searches only those unless `include_proposed` asks for the rest, rather
//! than returning names as facts; the gel says it is a model and not a
//! measurement. An assistant will repeat whatever it is handed, so anything
//! hedged in the terminal has to be hedged here too, or the hedge is lost
//! exactly where it matters most.
//!
//! That first sentence gave the reviewed count as "currently none" from
//! 2026-07-28 until 2026-08-09. It was written on the morning the sign-off
//! table was still empty and was false by that evening, when the rows were
//! signed — the same sentence this project has since corrected in `README.md`,
//! `features/SIGNOFF.tsv`, `Db::builtin`'s rustdoc and the `annotate` tool
//! schema below. No count is written here in its place, for the reason
//! `Db::builtin`'s rustdoc gives: ask `pl_features::Db::review_counts`, and do
//! not believe a count written into a doc comment. A `//!` block crosses no
//! wire, so what this misled was not an assistant but whoever opened the file
//! to check the project's central trust claim, and
//! `the_module_header_describes_the_database_that_ships` now reads it.
//!
//! The rule two paragraphs up — anything hedged in the terminal has to be
//! hedged here too — is not self-executing, and it has now been broken four
//! times. Three on 2026-07-28 (`AUDIT-2026-07-28` #67, #68 and #89: a truncated
//! fragment list with no truncation marker, a genetic code silently
//! substituted, an empty ORF result with no threshold to read it against) and
//! once on 2026-08-13, when `annotate` was found to be dropping two of the
//! three qualifiers `pl annotate` appends to a hit — including `crosses
//! origin`, without which the reversed coordinate pair of an origin-spanning
//! feature is not merely unhedged but unreadable. `annotate` now also heads its
//! reply with the molecule's length and topology, as the CLI does, because a
//! wrapped range means nothing without them. One qualifier is still terminal
//! only and is named where it is skipped rather than left to be rediscovered:
//! `Annotation::fusion_orf`. The moral each time is the same and it is a
//! process one — a new field on a result type is a new hedge to route, and this
//! file is downstream of every one of them.
//!
//! # No dependencies
//!
//! JSON-RPC 2.0 over stdio, with [`json`] doing the parsing. The correctness
//! crates take no dependencies and this is their front door.

mod json;

use std::io::{BufRead, Read, Write};

use json::{arr, obj, s, Value};

const PROTOCOL: &str = "2024-11-05";

/// The most a single JSON-RPC request line may be before it is refused. A
/// well-formed request is tiny; an unterminated or oversized line must not be
/// allowed to buffer without bound (and then 4x-amplify into a `Vec<char>` in
/// the parser).
const MAX_REQUEST: usize = 16 * 1024 * 1024;

fn main() {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut out = std::io::stdout();
    while let Ok(Some((bytes, overflowed))) = read_request(&mut reader, MAX_REQUEST) {
        if overflowed {
            let r = error_for(&Value::Null, -32700, "request line exceeds the size limit");
            let _ = writeln!(out, "{}", json::write(&r));
            let _ = out.flush();
            continue;
        }
        let Ok(line) = String::from_utf8(bytes) else {
            let r = error_for(&Value::Null, -32700, "request line is not valid UTF-8");
            let _ = writeln!(out, "{}", json::write(&r));
            let _ = out.flush();
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = match json::parse(&line) {
            // A panic in a parser below (the format readers are thousands of LOC
            // over hostile input) must degrade this one request, not kill the
            // long-lived server for every request after it. A notification still
            // gets no reply, even on panic.
            Ok(req) => {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(&req))) {
                    Ok(r) => r,
                    Err(_) => req.get("id").map(|id| {
                        error_for(id, -32603, "internal error while handling the request")
                    }),
                }
            }
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

/// Read one `\n`-terminated request line without ever buffering more than `cap`
/// bytes. Returns `Ok(None)` at EOF, or `Ok(Some((bytes, overflowed)))` where
/// `overflowed` marks a line that exceeded `cap` — its content is dropped and
/// the caller refuses it, so one long line cannot exhaust memory.
fn read_request(r: &mut impl BufRead, cap: usize) -> std::io::Result<Option<(Vec<u8>, bool)>> {
    let mut line = Vec::new();
    let mut overflowed = false;
    loop {
        let available = r.fill_buf()?;
        if available.is_empty() {
            return Ok(if line.is_empty() && !overflowed {
                None
            } else {
                Some((line, overflowed))
            });
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(i) => {
                if !overflowed && line.len() + i <= cap {
                    line.extend_from_slice(&available[..i]);
                } else {
                    overflowed = true;
                    line.clear();
                }
                r.consume(i + 1);
                return Ok(Some((line, overflowed)));
            }
            None => {
                let n = available.len();
                if !overflowed && line.len() + n <= cap {
                    line.extend_from_slice(available);
                } else {
                    overflowed = true;
                    line.clear();
                }
                r.consume(n);
            }
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
    // The topic enumeration is generated from `pl_doc::TOPICS` rather than
    // written out, because a second hand-written list goes out of date against
    // the first the next time a topic is added — and here it already had. The
    // string shipped nine of eleven names, missing `cloning` and `design`, on
    // the one surface where the enumeration IS the machine-readable contract:
    // an assistant that wants the methods paragraph for a primer *design*
    // finds no `design`, calls `methods(topic="primers")`, and gets a
    // well-formed methods paragraph about primer *binding sites* instead. The
    // GUI builds its list this way already; this is the same decision.
    let topics = pl_doc::TOPICS
        .iter()
        .map(|t| t.name)
        .collect::<Vec<_>>()
        .join(", ");
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
            // Split by topology rather than promising both invariances for
            // both, because only one of them is true of a linear molecule:
            // rotating a linear duplex makes a different molecule. The old
            // wording claimed rotation invariance for everything and the
            // linear branch did not even deliver strand invariance.
            "SEGUID v2 checksum. Circular: cdseguid, the same however the \
             molecule was rotated and whichever strand was written first. \
             Linear: ldseguid, the same whichever strand was written first. \
             The single-strand lsseguid is given too, labelled as covering \
             one strand and not the molecule.",
            vec![("path", "string", "Path to the file")],
            &["path"],
        ),
        tool(
            "annotate",
            // States the RULE, not a count. The previous wording said the
            // shipped database was "entirely unreviewed, so this returns
            // nothing unless include_proposed is set" — which was true of the
            // table this server was written against and false of the one it
            // ships with, where every row carries a curator. An assistant
            // reading it would have set include_proposed on every call to get
            // any answer at all, opting itself into exactly the unreviewed rows
            // the flag exists to keep out.
            "Find known features. Only rows a named curator has signed off are \
             searched unless include_proposed is set, and anything returned is a \
             suggestion to check against its cited accession rather than an \
             identification.",
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
            vec![("topic", "string", topics.as_str())],
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

/// An argument the caller actually supplied, or `None`.
///
/// [`Value::as_i64`], [`Value::as_str`] and [`Value::as_bool`] each return
/// `None` for two different situations — the key was absent, and the key was
/// present holding the wrong JSON type — and `unwrap_or(default)` collapses
/// them. A client that sent `{"min_aa": "1000"}`, a number written as a string
/// and a routine tool-call artifact, therefore got the *default* threshold of
/// 30, and the reply named no threshold at all, so nothing in it could be
/// checked against what was asked for. Absent still means the default; present
/// and unreadable is a tool error the model can see and correct itself from.
///
/// An explicit `null` counts as absent, because that is what a client sends
/// for an optional field it has no value for; it carries no requested value
/// that could be lost.
fn supplied<'a>(a: &'a Value, key: &str) -> Option<&'a Value> {
    match a.get(key) {
        None | Some(Value::Null) => None,
        v => v,
    }
}

/// A string argument. Absent means `""`; the wrong type is refused.
fn text_arg(a: &Value, key: &str) -> Result<String, String> {
    match supplied(a, key) {
        None => Ok(String::new()),
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{key} must be a string, not {}", json::write(v))),
    }
}

/// A boolean argument, or `None` when it was not supplied.
fn flag_arg(a: &Value, key: &str) -> Result<Option<bool>, String> {
    match supplied(a, key) {
        None => Ok(None),
        Some(v) => v
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!("{key} must be true or false, not {}", json::write(v))),
    }
}

/// A whole-number argument, or `None` when it was not supplied.
///
/// Refuses a fraction for the reason [`json`] gives — a caller who meant a
/// length and wrote `3.7` has made a mistake, and rounding it away hides that
/// — and refuses a magnitude past 2^53 for a second reason: beyond there a
/// `f64` no longer holds consecutive integers, so `{"min_aa": 1e19}` came back
/// as "no ORF of 9223372036854775807 aa or more", naming a threshold the
/// caller never sent.
fn whole_arg(a: &Value, key: &str) -> Result<Option<i64>, String> {
    // Past this a `f64` cannot name a particular integer.
    const EXACT: f64 = 9_007_199_254_740_992.0;
    let Some(v) = supplied(a, key) else {
        return Ok(None);
    };
    let whole = || format!("{key} must be a whole number, not {}", json::write(v));
    let n = v.as_f64().ok_or_else(whole)?;
    if !n.is_finite() || n.abs() >= EXACT {
        return Err(format!(
            "{key} is out of range: {} is past the largest whole number this can name",
            json::write(v)
        ));
    }
    v.as_i64().map(Some).ok_or_else(whole)
}

/// Load record 1 of a file, together with what else the file held.
fn load(path: &str) -> Result<(pl_core::Molecule, pl_fileio::LoadReport), String> {
    // The read-only server's one filesystem door, and the path is untrusted: the
    // assistant driving it can be steered by injected content. Refuse a
    // non-regular file — a FIFO would hang, `/dev/zero` would read forever — and
    // cap the bytes actually read (not a stat that a growing file outraces), the
    // way `pl-scan` bounds the same risk.
    const MAX_FILE: u64 = 512 * 1024 * 1024;
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path}: not a regular file"));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut data = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut data)
        .map_err(|e| format!("{path}: {e}"))?;
    if data.len() as u64 > MAX_FILE {
        return Err(format!("{path}: exceeds the {MAX_FILE}-byte size limit"));
    }
    pl_fileio::load_with_report(&data)
        .map(|(m, _, r)| (m, r))
        .map_err(|e| format!("{path}: {e}"))
}

/// The opening line for a reply about a file that held more than one record.
///
/// [`load`] returns record 1 and every tool below then answers about it as
/// though it were the file. The [`pl_fileio::LoadReport`] was bound to `_`, so
/// a 3-record GenBank whose second record has three EcoRI sites came back as
/// "EcoRI: 1 cut(s)" with no record count and no warning — a statement true of
/// one record, handed over the process boundary as a fact about the file. The
/// CLI prints this on every matching verb (`note_first_record_only`), and the
/// checksum tool is the sharp case: an identity claim whose scope has to
/// travel with it. Empty when nothing was left behind, so a complete answer
/// reads as one.
fn first_record_only(report: &pl_fileio::LoadReport) -> Vec<String> {
    if report.truncated() {
        vec![format!(
            "note: this file holds {} records; everything below describes only the first",
            report.records
        )]
    } else {
        Vec::new()
    }
}

/// How long the molecule is and whether its ends join, in one clause —
/// `"5386 bp circular"` — hedged for the two shapes where that plain sentence
/// would be a lie.
///
/// **One function because there must be one sentence.** This three-way split
/// grew inside `read_molecule` and stayed there while it had one caller. It
/// acquired a second on 2026-08-13, when `annotate` began heading its reply
/// with the same fact (AUDIT-2026-08-13 #5 — a wrapped coordinate range is not
/// interpretable without the length and the topology to read it against). A
/// copy would have meant one tool on this server describing a UGENE annotation
/// track as "0 bp linear" while the other, three hundred lines away, called it
/// an annotation track and refused to name a topology — two answers to one
/// question, from one process, about one file.
///
/// The hedges are the function, not decoration on it:
///
/// * `Molecule::len` counts the bases actually present, and a GenBank record
///   may declare `5386 bp` on its LOCUS line and carry an empty `ORIGIN`. The
///   plain form called such a plasmid "0 bp" — a claim about the molecule made
///   out of a fact about the file, and exactly what the GC field on the same
///   `read_molecule` line refuses to do. `pl info` hedges this case, so this
///   has to as well.
/// * A standalone annotation track — features, no bases, no declared length,
///   and in the UGENE export this matches no topology word on the LOCUS line
///   either — was reported as "0 bp linear", asserting both a length its own
///   features contradict and a topology the file never gave. The span here is
///   inferred from the features and says so. [`pl_core::Topology`] has no
///   unknown state, which is precisely why this branch must not print one.
fn extent_of(m: &pl_core::Molecule) -> String {
    if m.sequence_absent() {
        format!(
            "{} bp DECLARED, but this file carries no bases, {}",
            m.span(),
            m.topology.as_str()
        )
    } else if m.is_annotation_track() {
        format!(
            "an annotation track: coordinates and no bases, spanning {} bp by its features",
            m.annotation_span()
        )
    } else {
        format!("{} bp {}", m.len(), m.topology.as_str())
    }
}

fn call(params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("tools/call needs a name")?;
    let a = params.get("arguments").cloned().unwrap_or(Value::Null);
    // A bad argument is a *tool* error and not a protocol error — see
    // [`tool_error`] — so every `?` inside [`run`] lands here and is turned
    // into one, rather than telling the client the server itself is broken.
    Ok(run(name, &a).unwrap_or_else(tool_error))
}

fn run(name: &str, a: &Value) -> Result<Value, String> {
    Ok(match name {
        "read_molecule" => {
            let (m, report) = load(&text_arg(a, "path")?)?;
            let c = pl_core::Composition::of(&m.seq);
            let mut lines = first_record_only(&report);
            // Hedged, and hedged in [`extent_of`] rather than here, because
            // `annotate` prints the same clause and the two must not be able to
            // disagree about the same file. The reasons for each hedge — a
            // declared length with no bases, and a bases-less annotation track
            // with no topology to name — are written out there.
            let extent = extent_of(&m);
            lines.push(format!(
                "{extent}, GC {}, {} feature(s), {} primer(s)",
                // `None` when the molecule holds no unambiguous bases —
                // "0.0%" would be a claim about a sequence there is nothing
                // to say about.
                c.gc_percent()
                    .map(|g| format!("{g:.1}%"))
                    .unwrap_or_else(|| "unknown".into()),
                m.features.len(),
                m.primers.len()
            ));
            text_result(lines.join("\n"))
        }
        "digest" => {
            let path = text_arg(a, "path")?;
            let (m, report) = load(&path)?;
            // A digest of nothing is not "no cuts". `pl digest` exits with this
            // sentence and `pl_digest_json` returns it; this tool answered "no
            // cuts" for a record that declared 5386 bp and shipped none, which
            // reads as a fact about the plasmid rather than about the file.
            if m.seq.is_empty() {
                return Err(format!("{path}: no bases to digest"));
            }
            let wanted = text_arg(a, "enzymes")?;
            let names: Vec<&str> = wanted
                .split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .collect();
            // Every requested name is resolved before anything is digested.
            // Filtering `digest_all`'s output by name meant a name the table
            // does not hold simply matched nothing, and the empty result came
            // back as "no cuts" — a claim about the molecule manufactured out
            // of a gap in the database, byte-identical to a true negative.
            // DpnI is the case that bites: GATC occurs in essentially every
            // plasmid, it is the standard post-PCR template-removal step, and
            // it is not among the 58. All the misses are named at once, so a
            // caller who mistyped three does not have to iterate three times.
            let unknown: Vec<&str> = names
                .iter()
                .copied()
                .filter(|n| pl_enzymes::by_name(n).is_none())
                .collect();
            if !unknown.is_empty() {
                return Err(format!(
                    "no enzyme named {} in the built-in set of {}",
                    unknown.join(", "),
                    pl_enzymes::ENZYMES.len()
                ));
            }
            let mut cuts = Vec::new();
            for d in pl_enzymes::digest_all(&m) {
                if !names.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(d.enzyme.name))
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
                cuts.push(format!(
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
            // Only reachable now for names that resolved and genuinely did not
            // cut, which is the only thing this sentence can honestly mean.
            if cuts.is_empty() {
                cuts.push("no cuts".into());
            }
            let mut lines = first_record_only(&report);
            lines.extend(cuts);
            text_result(lines.join("\n"))
        }
        "melting_temperature" => {
            let m = pl_thermo::Method::default();
            let mut lines = vec![format!("Method: {}", m.describe())];
            for o in text_arg(a, "oligos")?
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
        "open_reading_frames" => {
            let (m, report) = load(&text_arg(a, "path")?)?;
            // Checked *before* it is narrowed, and reported as the caller
            // wrote it.
            //
            // `as u8` on the way in truncated to the low byte, so a request
            // for table 267 silently became table 11 and -243 became 13 —
            // both real NCBI codes, so the guard below never fired and the
            // ORFs came back computed under a genetic code nobody asked
            // for. Table 300 did reach the error, and named 44.
            //
            // `whole_arg` rather than `as_i64().unwrap_or(11)` because that
            // also read `{"table": "2"}` as "no table was given".
            let id = whole_arg(a, "table")?.unwrap_or(11);
            let Some(code) = u8::try_from(id).ok().and_then(pl_core::translate::table) else {
                return Err(format!("no NCBI code {id}"));
            };
            // Likewise: `-1 as usize` is 18,446,744,073,709,551,615, which
            // no ORF can reach, and the reply was then byte-identical to a
            // molecule that genuinely has no ORF at the threshold asked
            // for.
            let want = whole_arg(a, "min_aa")?.unwrap_or(30);
            let Ok(min_aa) = usize::try_from(want) else {
                return Err(format!("min_aa must be zero or more, not {want}"));
            };
            let p = pl_core::orf::Params {
                min_aa,
                ..Default::default()
            };
            let orfs = pl_core::orf::find_orfs(&m.seq, code, m.topology.is_circular(), &p);
            let mut lines = first_record_only(&report);
            // The threshold rides on the header rather than only on the empty
            // branch below. It used to appear in one line that fires only when
            // nothing was found, so a reply that *did* list ORFs named no
            // threshold anywhere and a caller could not tell the one they asked
            // for from a default that had been substituted for it.
            lines.push(format!("table {id} — {}, min {min_aa} aa", code.name()));
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
                // `start..end` is the ORF's extent only while `laps` is zero.
                // A 33-base ORF on a 19 bp circle reports start 5, end 18 — an
                // inclusive range of 14 bases, with start < end so nothing
                // looks wrong — and a reader who slices those coordinates gets
                // 14 bases of the wrong sequence. `Orf::bases` is always the
                // length, and printing it is the only way it survives into a
                // reader that cannot inspect the struct.
                lines.push(format!(
                    "{}..{} {} {} aa, {} bp, starts {}{}",
                    o.start,
                    o.end,
                    if o.strand == pl_core::Strand::Reverse {
                        "-"
                    } else {
                        "+"
                    },
                    o.aa_len,
                    o.bases(),
                    String::from_utf8_lossy(&o.start_codon),
                    if o.laps > 0 {
                        format!(
                            " (crosses origin, and wraps the whole molecule {} more time(s) — \
                             the range above is {} whole lap(s) short of the ORF)",
                            o.laps, o.laps
                        )
                    } else if o.wrapped {
                        " (crosses origin)".to_string()
                    } else {
                        String::new()
                    }
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
        "checksum" => {
            let (m, report) = load(&text_arg(a, "path")?)?;
            // SEGUID is defined over unambiguous *uppercase* DNA, and NCBI
            // writes `ORIGIN` in lowercase. `Molecule::seq` is case-preserved,
            // so the raw bytes went straight to the algorithm and a stock
            // GenBank download came back as `NotInAlphabet('g')` — an error
            // about the file's letter case, reported as though the molecule had
            // no checksum. The CLI upper-cases and says that it did.
            let seq = String::from_utf8_lossy(&m.seq).to_uppercase();
            let lower = m.seq.iter().filter(|b| b.is_ascii_lowercase()).count();
            let rc =
                String::from_utf8_lossy(&pl_core::reverse_complement(seq.as_bytes())).into_owned();
            // The *duplex* checksum is the identity claim, and it is the one
            // this tool's description promises: the same however the molecule
            // was rotated or whichever strand was written first. `lsseguid` is
            // neither — it covers one strand — so two files holding the same
            // linear duplex, one written as the reverse complement of the
            // other, used to get different checksums from a tool whose
            // description says they cannot. FASTA declares no topology and
            // loads as linear, so that was the branch the commonest input took.
            // `ldseguid` is what `pl checksum` prints for the linear case, and
            // its preconditions hold here by construction because `rc` is the
            // reverse complement of `seq`.
            let duplex = if m.topology.is_circular() {
                pl_core::cdseguid(&seq, &rc)
            } else {
                pl_core::ldseguid(&seq, &rc)
            };
            match duplex {
                Ok(x) => {
                    let mut lines = first_record_only(&report);
                    if lower > 0 {
                        lines.push(format!(
                            "note: {lower} lowercase base(s) upper-cased for the checksum"
                        ));
                    }
                    // No `"cdseguid: "` prefix in front of it: the SEGUID
                    // string already carries its own, and formatting a second
                    // one produced "lsseguid: lsseguid=…".
                    lines.push(x);
                    // Still offered, and hedged exactly as the terminal hedges
                    // it, because it identifies one strand and not the molecule.
                    match pl_core::lsseguid(&seq) {
                        Ok(v) => lines.push(format!("{v}   (this strand alone)")),
                        Err(e) => lines.push(format!("lsseguid: {e}")),
                    }
                    text_result(lines.join("\n"))
                }
                // A checksum over a sequence with a character the algorithm
                // does not define is not a checksum, and returning one
                // anyway is how two different molecules come to look equal.
                Err(e) => return Err(format!("no checksum for this sequence: {e}")),
            }
        }
        "annotate" => {
            let (m, report) = load(&text_arg(a, "path")?)?;
            let (all, _) = pl_features::Db::builtin();
            let proposed = flag_arg(a, "include_proposed")?.unwrap_or(false);
            let db = if proposed {
                all.clone()
            } else {
                all.reviewed()
            };
            let mut note = first_record_only(&report);
            // The molecule the coordinates below are coordinates *in*, said
            // once, at the head, before any of them.
            //
            // `pl annotate` heads its hit list with `{path}  {n} bp
            // circular|linear` (bins/pl/src/main.rs) and this tool's only
            // preamble was `first_record_only`, which is empty for the ordinary
            // single-record file. So the reply for an origin-spanning feature
            // was the bare pair `681..80` with no length and no topology, and
            // an assistant handed that cannot reconstruct the wrap even by
            // arithmetic: on a linear molecule `681..80` is a malformed range,
            // on an 800 bp circle it is a 200-base arc, and nothing in the
            // reply distinguished the two.
            //
            // The convention sentence rides along on the circular branch only.
            // On a linear molecule `start > end` is not a wrap but an error,
            // and printing the rule there would teach a reader to read a bug as
            // a feature.
            let mut head = extent_of(&m);
            if m.topology.is_circular() {
                head.push_str(
                    ". A range below whose start is greater than its end runs \
                     across base 1",
                );
            }
            note.push(head);
            if db.records.is_empty() {
                // The caveat has to survive the process boundary. An
                // assistant repeats what it is handed, so "nothing found"
                // without the reason would be read as "this plasmid has no
                // known features".
                let mut lines = note;
                lines.push(format!(
                    "Nothing was searched: {} of {} database records have been \
                     reviewed by a named curator. The rest were assembled by \
                     machine from public sources and are not used by default. \
                     Set include_proposed to search them, and treat anything \
                     found as a suggestion to check against its cited accession.",
                    all.reviewed().records.len(),
                    all.records.len()
                ));
                return Ok(text_result(lines.join("\n")));
            }
            let ann = pl_features::annotate::Annotator::new(
                &db,
                pl_features::annotate::Config::default(),
            );
            let found = ann.annotate(&m);
            let mut lines: Vec<String> = note;
            lines.extend(found.iter().map(|f| {
                // Every qualifier the terminal prints, or the qualifier is lost
                // exactly where it matters most.
                //
                // `pl annotate` appends "  fragment", "  via protein" and
                // "  crosses origin" to its hit lines; only the first of the
                // three survived into this reply, and `wraps_origin` was read
                // nowhere in this crate at all. The cost is not cosmetic,
                // because `wraps_origin` is the only thing that makes the
                // coordinates legible: `Annotation` guarantees `start > end`
                // exactly when it is set. `annotate.rs`'s own fixture measures
                // the shape — an 800 bp circle carrying a 200 bp record across
                // base 1 reports `681..80` — and an assistant handed that
                // either relays "681 to 80" or silently normalises it to
                // "80..681", the only reading that parses as an interval, and
                // that arc holds none of the 200 bases the feature occupies.
                // Someone then orders sequencing primers against it.
                // `via_protein` is milder and still a real claim: a hit found
                // only by translation says the nucleotides were rewritten,
                // which is what a user needs to know before designing a probe
                // or an allele-specific primer against the coordinates.
                //
                // Second sweep of this file for this one class, and the fourth
                // instance of it: AUDIT-2026-07-28 #67 (the digest fragment
                // cap), #68 and #89 (the ORF table and the ORF threshold) were
                // all "a hedge the CLI prints that does not survive the process
                // boundary", and the `annotate` suffix flags were simply
                // missed. One flag is still missed on purpose and named here
                // rather than left to be rediscovered:
                // `Annotation::fusion_orf`, the ORF a peptide part was admitted
                // on, which `pl annotate` prints on a continuation line. It is
                // outside AUDIT-2026-08-13 #5's stated fix and has no test on
                // this surface yet; do not read its absence below as evidence
                // that nothing needs it.
                //
                // Accumulated rather than formatted inline so the order is
                // fixed in one place and matches the CLI's, and so a fourth
                // qualifier is one statement rather than another `{}` counted
                // off against a widening argument list.
                let mut hedges = String::new();
                if f.is_fragment {
                    hedges.push_str(", fragment");
                }
                if f.via_protein {
                    hedges.push_str(", via protein");
                }
                if f.wraps_origin {
                    hedges.push_str(", crosses origin");
                }
                format!(
                    "{}..{} {} {} — {:.1}% identity, {:.0}% coverage{hedges}",
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
                )
            }));
            if found.is_empty() {
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
        "methods" => match pl_doc::topic(&text_arg(a, "topic")?) {
            Some(t) => text_result(pl_doc::methods(t)),
            None => {
                return Err(format!(
                    "unknown topic; try one of {}",
                    pl_doc::TOPICS
                        .iter()
                        .map(|t| t.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        },
        other => return Err(format!("unknown tool {other:?}")),
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
    /// Named per process **and per call**. The per-process half was here first
    /// and closed the race between two test binaries; the per-call half closes
    /// the one inside a single binary, which is the one that actually bit.
    /// `cargo test` runs a suite on several threads, four tests here ask for a
    /// fixture called `orf.fa`, and `fs::write` truncates before it writes — so
    /// one test can open the file in the instant another has emptied it and get
    /// "unrecognised format -- expected SnapGene .dna, GenBank or FASTA".
    ///
    /// Observed **once**, during a `cargo test --workspace` run, as
    /// `a_genetic_code_outside_a_byte_is_refused_rather_than_wrapped` failing
    /// with an error that named neither a genetic code nor a race. Fifteen
    /// consecutive runs of this suite alone did not reproduce it, which bounds
    /// the window as narrow rather than showing it is not there: the four
    /// writes carry identical bytes, so the only losing interleaving is a read
    /// landing between the truncate and the write. The fix rests on that
    /// structural argument and not on a frequency, because one observation
    /// cannot support a frequency. A test that fails at random is worse than a
    /// missing one — it teaches whoever reads the suite to re-run until green.
    fn fixture(name: &str, text: &str) -> String {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NTH: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("pl-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp directory");
        let p = dir.join(format!("{}-{name}", NTH.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&p, text).expect("a fixture");
        p.display().to_string()
    }

    /// Call one tool and return the text it replied with, and whether the reply
    /// was flagged `isError`.
    fn call_tool_full(name: &str, args: Vec<(&str, Value)>) -> (String, bool) {
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
        let res = r.get("result").unwrap();
        let text = res.get("content").unwrap().as_array().unwrap()[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let failed = res.get("isError").and_then(Value::as_bool).unwrap_or(false);
        (text, failed)
    }

    /// Call one tool and return the text it replied with, error or not.
    fn call_tool(name: &str, args: Vec<(&str, Value)>) -> String {
        call_tool_full(name, args).0
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

    /// The `methods` tool's schema must enumerate every topic `pl-doc` has, and
    /// the `annotate` tool must not describe the shipped database as unreviewed.
    ///
    /// PROVEN TO FAIL at 713bd3b, on both arms:
    ///
    /// * the topic parameter advertised "tm, digest, gel, orfs, sanger,
    ///   annotate, checksum, goldengate or primers" — 9 of `TOPICS`' 11, with
    ///   `cloning` and `design` missing. This is the one surface where the
    ///   enumeration *is* the machine-readable contract, and the failure is
    ///   silent rather than loud: an assistant asked for the methods paragraph
    ///   for a primer *design*, finding no `design`, calls
    ///   `methods(topic="primers")` and gets the primer *binding site*
    ///   paragraph — a well-formed methods paragraph about a different
    ///   operation. The GUI already builds its list from `TOPICS` itself, with
    ///   a comment saying a second hand-written list "would go out of date
    ///   against the first the next time a topic is added". It did.
    /// * `annotate` said "The shipped database is entirely unreviewed, so this
    ///   returns nothing unless include_proposed is set". Measured against the
    ///   compiled-in tables: 89 of 89 rows are signed, so the default call
    ///   returns 89 records' worth of hits and the sentence told the assistant
    ///   the opposite of what the tool does.
    ///
    /// Both arms read the description out of the live schema rather than out of
    /// a copy here, so the check tracks the contract a client actually sees.
    #[test]
    fn the_schema_descriptions_match_what_the_tools_do() {
        let r = req(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        let tools = r
            .get("result")
            .unwrap()
            .get("tools")
            .unwrap()
            .as_array()
            .unwrap();
        let described = |tool: &str, prop: &str| -> String {
            let t = tools
                .iter()
                .find(|t| t.get("name").unwrap().as_str() == Some(tool))
                .unwrap_or_else(|| panic!("no {tool} tool"));
            let v = if prop.is_empty() {
                t.get("description").unwrap()
            } else {
                t.get("inputSchema")
                    .unwrap()
                    .get("properties")
                    .unwrap()
                    .get(prop)
                    .unwrap_or_else(|| panic!("{tool} declares no {prop}"))
                    .get("description")
                    .unwrap()
            };
            v.as_str().unwrap().to_string()
        };

        let topics = described("methods", "topic");
        for t in pl_doc::TOPICS {
            assert!(
                topics.contains(t.name),
                "the methods schema does not offer {:?}, so a client asking for \
                 it has to guess: {topics:?}",
                t.name
            );
        }

        let annotate = described("annotate", "");
        let (db, _) = pl_features::Db::builtin();
        let signed = db.reviewed().records.len();
        assert!(
            signed > 0,
            "if the shipped database really is unsigned again, this test and \
             the annotate description both describe the new state"
        );
        assert!(
            !annotate.contains("entirely unreviewed"),
            "the annotate schema calls the database entirely unreviewed while \
             {signed} of {} rows are signed: {annotate:?}",
            db.records.len()
        );
    }

    /// Does `text` make `claim` in its own voice, rather than quoting it?
    ///
    /// An occurrence whose nearest preceding non-space character opens a
    /// quotation — `"`, `“` or a backtick — is somebody being quoted, and a
    /// correction that says what a sentence *used* to say has to be able to
    /// print the old sentence. The same six lines are in
    /// `crates/pl-scan/src/lib.rs` and `crates/pl-doc/src/lib.rs`, which guard
    /// the same class of stale claim in their own files; sharing them would
    /// mean a new workspace member existing only to hold one predicate.
    fn asserts(text: &str, claim: &str) -> bool {
        text.match_indices(claim).any(|(i, _)| {
            !matches!(
                text[..i].chars().rev().find(|c| !c.is_whitespace()),
                Some('"') | Some('\u{201c}') | Some('`')
            )
        })
    }

    /// The module header describes the database that ships, not the one that
    /// shipped the morning it was written.
    ///
    /// PROVEN TO FAIL at c44757b:
    ///
    /// ```text
    /// the module header says "currently none" while 89 of 89 rows are signed
    /// ```
    ///
    /// [`the_schema_descriptions_match_what_the_tools_do`] above reads the live
    /// tool schema, which is why the same sentence was found and corrected
    /// there — it is the second arm of that test's own PROVEN TO FAIL record,
    /// against `713bd3b` — and left standing 940 lines higher, in this file's
    /// front matter, where nothing looked. A `//!` block never crosses the
    /// wire, so no assistant was misled
    /// by it — the reader it misleads is whoever opens this file to check the
    /// project's central trust claim, which is what a front matter is for.
    ///
    /// Whitespace-normalised before searching, because the claim wrapped across
    /// two comment lines — "reviewed — currently" / "none — rather than" — and
    /// a line-by-line `contains` would never have seen it.
    #[test]
    fn the_module_header_describes_the_database_that_ships() {
        const SELF: &str = include_str!("main.rs");
        let (db, errors) = pl_features::Db::builtin();
        assert!(errors.is_empty(), "{errors:?}");
        let signed = db.reviewed().records.len();
        assert!(
            signed > 0,
            "the premise: if the shipped tables really are unsigned again, this \
             test and the header it constrains both describe the new state"
        );

        let header: String = SELF
            .lines()
            .take_while(|l| l.starts_with("//!") || l.trim().is_empty())
            // The `//!` goes before the join, not after: leaving it in puts a
            // marker between every pair of words that wrapped, and this claim
            // wrapped. A first draft of this test kept the prefixes, found
            // "currently //! none" where it was looking for "currently none",
            // and passed against the very sentence it was written for.
            .map(|l| l.trim_start_matches("//!"))
            .collect::<Vec<_>>()
            .join(" ");
        let header: String = header.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            header.contains("read-only"),
            "the header no longer starts where this test thinks it does: \
             {header}"
        );
        for claim in [
            "currently none",
            "nothing reviewed",
            "none reviewed",
            "0 reviewed",
            "no reviewed records",
            "entirely unreviewed",
        ] {
            assert!(
                !asserts(&header, claim),
                "the module header says {claim:?} while {signed} of {} records \
                 carry a curator sign-off",
                db.records.len()
            );
        }
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

    /// `PLF:0001` — AmpR, 861 bases — as the shipped database actually holds
    /// it, plus the name the reply will print for it.
    ///
    /// Read out of [`pl_features::Db::builtin`] rather than pasted in, because
    /// the tool searches `Db::builtin().reviewed()` and a pasted copy would
    /// silently stop being the thing under test the first time the row is
    /// re-derived. Panics loudly if the row is gone or unsigned: that is a fact
    /// about the shipped database worth failing over, not a reason to skip.
    ///
    /// AmpR because it is the audit's own failure scenario and the commonest
    /// real one — the numbering origin of an assembled plasmid is wherever the
    /// assembler cut the circle, so a full-length marker straddling base 1 is
    /// routine rather than exotic.
    fn shipped_ampr() -> (String, String) {
        let (all, _) = pl_features::Db::builtin();
        let db = all.reviewed();
        let r = db
            .records
            .iter()
            .find(|r| r.id == "PLF:0001")
            .expect("PLF:0001 is signed off and clears the taint gate");
        let raw = r.reference_nt.clone();
        let nt = String::from_utf8(raw).expect("bases are ASCII");
        assert_eq!(nt.len(), 861, "PLF:0001 changed length: {}", nt.len());
        (r.name.clone(), nt)
    }

    /// Deterministic filler DNA, so the fixtures below carry the same bases on
    /// every machine and every run.
    ///
    /// xorshift64, five lines, no dependency — `crates/` takes none and this
    /// binary matches them. A `SystemTime`-seeded generator here would make a
    /// failure unreproducible, and `fixture`'s own doc comment records what a
    /// test that fails at random costs: it teaches whoever reads the suite to
    /// re-run until green.
    fn filler(n: usize, seed: u64) -> String {
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b"ACGT"[(x >> 33) as usize % 4] as char
            })
            .collect()
    }

    /// A one-record circular GenBank file carrying exactly `seq`.
    fn circular_gb(name: &str, seq: &str) -> String {
        fixture(
            &format!("{name}.gb"),
            &format!(
                "LOCUS       {name}    {} bp    DNA     circular SYN 01-JAN-2026\n\
                 ORIGIN\n        1 {seq}\n//\n",
                seq.len()
            ),
        )
    }

    /// The reply line for the hit on `name`, or a panic showing the whole
    /// reply — which is the only useful failure message when the hit is the
    /// thing that went missing.
    fn hit_line<'a>(text: &'a str, name: &str) -> &'a str {
        text.lines()
            .find(|l| l.contains("% identity") && l.contains(name))
            .unwrap_or_else(|| panic!("no hit on {name} in {text:?}"))
    }

    /// The `start..end` a hit line opens with.
    fn range_of(line: &str) -> (u64, u64) {
        let head = line.split(' ').next().expect("a first field");
        let (a, b) = head
            .split_once("..")
            .unwrap_or_else(|| panic!("no range at the head of {line:?}"));
        (
            a.parse().unwrap_or_else(|_| panic!("start of {line:?}")),
            b.parse().unwrap_or_else(|_| panic!("end of {line:?}")),
        )
    }

    /// PROVEN TO FAIL at f0e4a6f: the `annotate` tool's hit format string
    /// filled its one trailing slot from `is_fragment` alone and read
    /// `wraps_origin` nowhere, and the reply's only preamble was
    /// `first_record_only`, which is empty for a single-record file. So a
    /// full-length AmpR straddling base 1 of a 1,461 bp circle crossed the
    /// process boundary as the bare pair `1062..400` — start greater than end,
    /// with no note, no molecule length and no topology. An assistant either
    /// relays "1062 to 400" or normalises it to "400 to 1062", the only reading
    /// that parses as an interval, and that arc holds none of the 861 bases the
    /// feature occupies. At f0e4a6f two of the three assertions below fail —
    /// `1461 bp circular` and `, crosses origin`. The `start > end` one is the
    /// premise and not the claim: it holds at f0e4a6f too, because the
    /// annotator was always right and only the rendering was not, and it is
    /// here so that a fixture which quietly stopped wrapping cannot leave this
    /// test passing on an empty condition.
    ///
    /// TO RE-BREAK IT: in the `"annotate"` arm of [`run`], delete the
    /// `if f.wraps_origin { hedges.push_str(", crosses origin"); }` statement.
    /// (Deleting the `note.push(head);` a few lines above it re-breaks the
    /// header half on its own.)
    ///
    /// # Why a real record and not a synthetic one
    ///
    /// `crates/pl-features/src/annotate.rs`'s own
    /// `a_feature_spanning_the_origin_of_a_circle_is_found_whole` proves the
    /// same shape against a hand-built database, and proves it at the level of
    /// the `Annotation` struct — where the flag was never in doubt. What was in
    /// doubt is whether the flag survives being turned into text and handed to
    /// another process, and that question can only be asked of the database the
    /// server actually searches.
    #[test]
    fn a_hit_across_the_origin_says_so_across_the_process_boundary() {
        let (name, amp) = shipped_ampr();
        // The last 461 bases of AmpR at the start of the file and the first 400
        // at the end, so the gene runs across base 1 exactly as it does on a
        // plasmid the assembler happened to linearise inside the marker.
        let mid = filler(600, 0x5157_2026_0813);
        let seq = format!("{}{mid}{}", &amp[400..], &amp[..400]);
        assert_eq!(seq.len(), 1461);
        let path = circular_gb("wrapped", &seq);
        let text = call_tool("annotate", vec![("path", s(path))]);
        assert!(
            text.contains("1461 bp circular"),
            "no length or topology to read the range against: {text}"
        );
        let line = hit_line(&text, &name);
        let (start, end) = range_of(line);
        assert!(
            start > end,
            "the fixture did not produce a wrapped hit, so this test proves \
             nothing: {line}"
        );
        assert!(
            line.contains(", crosses origin"),
            "a reversed range crossed the boundary with nothing to explain it: \
             {line}"
        );
    }

    /// The control for
    /// [`a_hit_across_the_origin_says_so_across_the_process_boundary`], and the
    /// half that makes the pair falsifiable in both directions: the same
    /// marker, the same filler and the same 1,461 bp circumference, with the
    /// marker moved clear of base 1.
    ///
    /// PROVEN TO FAIL against the obvious wrong fix — appending
    /// ", crosses origin" unconditionally, or keying it on anything other than
    /// `Annotation::wraps_origin` — which the wrapped test alone cannot
    /// distinguish from the right one. A wrap note on a feature that does not
    /// wrap sends a reader looking for bases that are not there, which is the
    /// same class of harm in the other direction.
    ///
    /// TO RE-BREAK IT: in the `"annotate"` arm of [`run`], change
    /// `if f.wraps_origin` to `if true` (or drop the `if` and push the string
    /// unconditionally).
    #[test]
    fn a_hit_that_does_not_cross_the_origin_is_not_said_to() {
        let (name, amp) = shipped_ampr();
        let f = filler(600, 0x5157_2026_0813);
        let seq = format!("{}{amp}{}", &f[..300], &f[300..]);
        assert_eq!(seq.len(), 1461);
        let path = circular_gb("interior", &seq);
        let text = call_tool("annotate", vec![("path", s(path))]);
        assert!(text.contains("1461 bp circular"), "{text}");
        let line = hit_line(&text, &name);
        let (start, end) = range_of(line);
        assert!(start < end, "the control wrapped after all: {line}");
        assert!(
            !line.contains("crosses origin"),
            "a wrap note on a feature that does not wrap: {line}"
        );
    }

    /// PROVEN TO FAIL at f0e4a6f: `Annotation::via_protein` was read nowhere in
    /// this crate, so a marker found *only* by six-frame translation — i.e. one
    /// whose nucleotides have been rewritten past the 96% identity gate, which
    /// is what "codon-optimised" means — was reported in exactly the same words
    /// as a nucleotide-identical one. The `100.0% identity` on that line is a
    /// protein identity, and without the qualifier a reader designing a probe,
    /// a diagnostic digest or an allele-specific primer against those
    /// coordinates has been told the bases match when only the residues do.
    /// `pl annotate` appends "  via protein"; this reply did not.
    ///
    /// TO RE-BREAK IT: in the `"annotate"` arm of [`run`], delete the
    /// `if f.via_protein { hedges.push_str(", via protein"); }` statement.
    #[test]
    fn a_marker_found_only_by_its_protein_says_which_alphabet_matched() {
        let (name, amp) = shipped_ampr();
        // Recode AmpR by choosing the LAST synonymous codon available for each
        // residue, the same construction
        // `crates/pl-features/src/annotate.rs`'s
        // `a_codon_optimised_gene_is_found_by_its_protein` uses: the protein is
        // untouched and the nucleotides look nothing like the reference, so the
        // DNA route cannot reach it and the translated route must.
        let code = pl_core::translate::TABLE11;
        // 861 bases is 287 codons and the last of them is the stop, which
        // `Code::translate` emits as `*` rather than truncating at.
        let full = code.translate(amp.as_bytes());
        let protein: Vec<u8> = match full.strip_suffix(b"*") {
            Some(p) => p.to_vec(),
            None => full.clone(),
        };
        assert_eq!(protein.len(), 286, "PLF:0001 is a 286-residue protein");
        let mut cds = String::new();
        for aa in protein.iter().copied() {
            let mut chosen = None;
            for b1 in b"TCAG" {
                for b2 in b"TCAG" {
                    for b3 in b"TCAG" {
                        let c = [*b1, *b2, *b3];
                        if code.codon(&c) == aa {
                            chosen = Some(c);
                        }
                    }
                }
            }
            let c = chosen.expect("every residue of a real protein has a codon");
            cds.push_str(std::str::from_utf8(&c).expect("a codon is ASCII"));
        }
        assert_eq!(code.translate(cds.as_bytes()), protein);
        let f = filler(800, 0x5654_2026_0813);
        let path = fixture(
            "recoded.fa",
            &format!(">recoded\n{}{cds}{}\n", &f[..400], &f[400..]),
        );
        let text = call_tool("annotate", vec![("path", s(path))]);
        let line = hit_line(&text, &name);
        assert!(
            line.contains(", via protein"),
            "a translated-only hit reads as a nucleotide identification: {line}"
        );
        // And the control on the same line: a recoded gene is still whole, so
        // the qualifier that IS printed today must not have quietly become the
        // one being asserted.
        assert!(!line.contains(", fragment"), "{line}");
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

    /// A temperature with no model behind it is not a result somebody can put
    /// in a paper — and, until 2026-08-13, only the model half of that sentence
    /// was checked.
    ///
    /// PROVEN TO FAIL at f0e4a6f: not as shipped, but against three mutations
    /// that f0e4a6f's version of this test passes, which is the same defect.
    /// Its assertions were `starts_with("Method:")` and
    /// `contains("GTAAAACGACGGCCAGT")`, and **both branches of the handler open
    /// by echoing the oligo** — `"{o}  {:.1} C  ({} nt)"` on success and
    /// `"{o}  cannot be computed: {e:?}"` on failure. So the second assertion
    /// was satisfied by the input coming back, not by an answer, and nothing
    /// anywhere looked at `Tm::tm`. Green under all three of: dropping the
    /// number from the format string; swapping the match arms so every oligo
    /// takes the error path; and substituting `t.gc_percent`, `t.dh` or `t.ds`,
    /// all `f64`, all of which fill `{:.1}` without a compile error and one of
    /// which — `gc_percent` for the M13 forward primer — prints a thoroughly
    /// plausible 52.9 "C" that a bench scientist would set a thermocycler from.
    /// The number is pinned on the CLI (`bins/pl/tests/cli.rs`) and inside
    /// `pl-thermo`, but neither of those touches the `format!` literal that is
    /// unique to this surface, and this is the one shipped surface whose output
    /// a user never reads directly: they read an assistant's paraphrase of it.
    ///
    /// The expected lines are built **from the model**, not written down as
    /// 57.8 and 38.9, so the assertion cannot drift away from the
    /// implementation the way a transcribed constant does — if `pl-thermo`'s
    /// parameters legitimately change, this test follows them, and if the MCP
    /// handler stops printing what the model says, it does not.
    ///
    /// TO RE-BREAK IT: in the `"melting_temperature"` arm of [`run`], change
    /// `t.tm` to `t.gc_percent` in the success `format!`.
    #[test]
    fn a_tm_request_reports_the_method_alongside_the_number() {
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
        let method = pl_thermo::Method::default();
        // Named, so the reply cannot claim one model and compute another.
        let head = format!("Method: {}", method.describe());
        assert_eq!(text.lines().next(), Some(head.as_str()), "{text}");
        // M13 forward, and a 4-mer that is above `pl_thermo`'s two-base floor
        // and so must take the success branch as well — the error branch is the
        // one the old assertions could not tell apart from a result.
        let m13 = b"GTAAAACGACGGCCAGT";
        let long = pl_thermo::tm(m13, &method).expect("M13 forward has a Tm");
        let short = pl_thermo::tm(b"ACGT", &method).expect("a computable 4-mer");
        let first = format!("GTAAAACGACGGCCAGT  {:.1} C  (17 nt)", long.tm);
        let second = format!("ACGT  {:.1} C  (4 nt)", short.tm);
        assert!(text.contains(first.as_str()), "want {first:?} in {text:?}");
        assert!(
            text.contains(second.as_str()),
            "want {second:?} in {text:?}"
        );
        assert!(
            !text.contains("cannot be computed"),
            "both oligos are computable and one was refused: {text}"
        );
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
        for n in [267.0, -243.0, 300.0] {
            let text = call_tool(
                "open_reading_frames",
                vec![("path", s(path.clone())), ("table", Value::Number(n))],
            );
            assert!(
                text.starts_with("no NCBI code") && text.contains(&format!("{}", n as i64)),
                "table {n} gave {text}"
            );
        }
        // 1e19 used to reach the same message, which then named
        // 9,223,372,036,854,775,807 — `n as i64` saturating to `i64::MAX`, a
        // number nobody sent. Past 2^53 a JSON number no longer identifies a
        // particular integer, so it is refused as out of range instead of being
        // echoed back as though it did.
        let text = call_tool(
            "open_reading_frames",
            vec![("path", s(path)), ("table", Value::Number(1e19))],
        );
        assert!(text.contains("table is out of range"), "{text}");
        assert!(!text.contains("9223372036854775807"), "{text}");
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

    /// 60 bp of ordinary sequence, used both ways round.
    const SIXTY: &str = "GCTAAAGACAATTACATAACATACACGTCAGCACGAAACTTGTTGGCCCAGTGTGAATCG";

    fn revcomp(seq: &str) -> String {
        String::from_utf8(pl_core::reverse_complement(seq.as_bytes())).expect("ASCII")
    }

    #[test]
    fn one_linear_duplex_written_either_way_round_gets_one_checksum() {
        // The tool's own description promises a checksum that is "the same for
        // the same molecule however it was rotated or which strand was written
        // first". The linear branch returned `lsseguid`, which covers ONE
        // STRAND and is not strand-invariant by construction, so two files
        // holding the same duplex — one written as the reverse complement of
        // the other — came back with different checksums under a description
        // saying they could not. FASTA declares no topology and loads as
        // linear, so this was the branch the commonest input took, and an
        // assistant relayed "these are two different molecules".
        let fwd = call_tool(
            "checksum",
            vec![("path", s(fixture("fwd.fa", &format!(">f\n{SIXTY}\n"))))],
        );
        let rev = call_tool(
            "checksum",
            vec![(
                "path",
                s(fixture("rev.fa", &format!(">r\n{}\n", revcomp(SIXTY)))),
            )],
        );
        let duplex = |t: &str| {
            t.lines()
                .find(|l| l.starts_with("ldseguid="))
                .unwrap_or_else(|| panic!("no duplex checksum in {t:?}"))
                .to_string()
        };
        assert_eq!(
            duplex(&fwd),
            duplex(&rev),
            "one duplex, two checksums:\n{fwd}\n---\n{rev}"
        );
        // The single-strand value is still offered and still differs — it has
        // to carry the qualifier the terminal prints, or it reads as the
        // molecule's identity.
        assert!(fwd.contains("(this strand alone)"), "{fwd}");
        assert_ne!(
            fwd.lines().find(|l| l.starts_with("lsseguid=")),
            rev.lines().find(|l| l.starts_with("lsseguid=")),
        );
        // The SEGUID string carries its own prefix; a second one made
        // "lsseguid: lsseguid=…".
        assert!(!fwd.contains("lsseguid: lsseguid="), "{fwd}");
        assert!(!fwd.contains("ldseguid: ldseguid="), "{fwd}");
    }

    #[test]
    fn a_circular_checksum_is_still_the_rotation_invariant_one() {
        // The control. The circular branch was right and must stay right.
        let gb = |name: &str, seq: &str| {
            fixture(
                name,
                &format!(
                    "LOCUS       c                       60 bp    DNA     circular SYN 01-JAN-2026\n\
                     ORIGIN\n        1 {seq}\n//\n"
                ),
            )
        };
        let fwd = call_tool("checksum", vec![("path", s(gb("cf.gb", SIXTY)))]);
        let rev = call_tool("checksum", vec![("path", s(gb("cr.gb", &revcomp(SIXTY))))]);
        let cd = |t: &str| {
            t.lines()
                .find(|l| l.starts_with("cdseguid="))
                .unwrap_or_else(|| panic!("no circular checksum in {t:?}"))
                .to_string()
        };
        assert_eq!(cd(&fwd), cd(&rev), "{fwd}\n---\n{rev}");
    }

    #[test]
    fn a_lowercase_record_is_checksummed_rather_than_refused() {
        // SEGUID is defined over uppercase DNA and `Molecule::seq` is
        // case-preserved, so a stock NCBI download — `ORIGIN` is written in
        // lowercase — came back as an `isError` reply reading "no checksum for
        // this sequence: NotInAlphabet('g')", an error about letter case
        // presented as the molecule having no checksum. `pl checksum`
        // upper-cases and says how many bases it touched.
        let path = fixture(
            "lower.gb",
            &format!(
                "LOCUS       l                       60 bp    DNA     linear   SYN 01-JAN-2026\n\
                 ORIGIN\n        1 {}\n//\n",
                SIXTY.to_lowercase()
            ),
        );
        let (text, failed) = call_tool_full("checksum", vec![("path", s(path))]);
        assert!(!failed, "a lowercase record has a checksum: {text}");
        assert!(text.contains("ldseguid="), "{text}");
        assert!(
            text.contains("60 lowercase base(s) upper-cased"),
            "the fold has to be stated, not silent: {text}"
        );
    }

    /// Three FASTA records in one file: one EcoRI site, then three, then none.
    fn three_records() -> String {
        let a = format!("GAATTC{}", "A".repeat(54));
        let b = format!("GAATTC{a14}GAATTC{a14}GAATTC{a14}", a14 = "A".repeat(14));
        let c = "ACGT".repeat(15);
        fixture("multi.fa", &format!(">recA\n{a}\n>recB\n{b}\n>recC\n{c}\n"))
    }

    #[test]
    fn a_file_holding_more_than_one_record_says_so_on_every_tool() {
        // `load_with_report` returns the report and the shared closure bound it
        // to `_`, so every path tool answered about record 1 as though it were
        // the file. "EcoRI: 1 cut(s)" is true of record 1 and false of the file
        // — record 2 has three sites — and an assistant relayed "EcoRI is a
        // single cutter in this plasmid". The CLI prints the count on every one
        // of the matching verbs.
        let path = three_records();
        for (tool, args) in [
            ("read_molecule", vec![]),
            ("digest", vec![("enzymes", s("EcoRI"))]),
            ("checksum", vec![]),
            ("open_reading_frames", vec![("min_aa", Value::Number(1.0))]),
            ("annotate", vec![]),
        ] {
            let mut all = vec![("path", s(path.clone()))];
            all.extend(args);
            let text = call_tool(tool, all);
            assert!(
                text.contains("this file holds 3 records"),
                "{tool} answered about record 1 as though it were the file: {text}"
            );
        }
        // And the digest answer it qualifies is still the record-1 one.
        let text = call_tool("digest", vec![("path", s(path)), ("enzymes", s("EcoRI"))]);
        assert!(text.contains("1 cut(s)"), "{text}");
    }

    #[test]
    fn a_single_record_file_is_not_told_it_holds_more() {
        // The control. A note on a file that lost nothing would be its own lie.
        let text = call_tool(
            "read_molecule",
            vec![("path", s(fixture("one.fa", &format!(">one\n{SIXTY}\n"))))],
        );
        assert!(!text.contains("records"), "{text}");
    }

    #[test]
    fn an_enzyme_the_table_does_not_hold_is_refused_rather_than_reported_as_no_cuts() {
        // The requested names were only ever used to filter `digest_all`'s
        // output, so a name absent from the 58 matched nothing and the empty
        // result was reported as "no cuts" — a claim about the molecule
        // manufactured out of a gap in the database. DpnI is the case that
        // bites: GATC is in essentially every plasmid, it is the standard
        // post-PCR template-removal step, and it is not in the table. The
        // fixture below physically contains three GATC and three GGCC sites.
        let path = fixture(
            "sites.fa",
            ">sites\nGGCCTTTTGATCAAAAGGCCTTTTGATCAAAAGGCCTTTTGATCAAAAGAATTCTTTTAAAA\n",
        );
        for name in ["DpnI", "HaeIII", "EcoR1"] {
            let (text, failed) = call_tool_full(
                "digest",
                vec![("path", s(path.clone())), ("enzymes", s(name))],
            );
            assert!(failed, "{name} was not refused: {text}");
            assert!(text.contains(name), "{text}");
            assert!(
                !text.contains("no cuts"),
                "an unknown name became a negative result: {text}"
            );
        }
        // Every miss is named at once, so three typos need one round trip.
        let (text, _) = call_tool_full(
            "digest",
            vec![
                ("path", s(path.clone())),
                ("enzymes", s("DpnI, HaeIII, EcoRI")),
            ],
        );
        assert!(text.contains("DpnI") && text.contains("HaeIII"), "{text}");
        // The control: a name the table DOES hold, with no site here, is still
        // "no cuts", and one with a site still cuts.
        let (text, failed) = call_tool_full(
            "digest",
            vec![("path", s(path.clone())), ("enzymes", s("NotI"))],
        );
        assert!(!failed && text == "no cuts", "{text}");
        let text = call_tool("digest", vec![("path", s(path)), ("enzymes", s("EcoRI"))]);
        assert!(text.contains("EcoRI: 1 cut(s)"), "{text}");
    }

    /// A GenBank record declaring 5,386 bp with an `ORIGIN` that carries none.
    /// Real and common — see `pl_core::Molecule::declared_len`.
    fn declared_but_absent() -> String {
        fixture(
            "absent.gb",
            "LOCUS       pBIG                  5386 bp    DNA     circular SYN 01-JAN-2026\n\
             FEATURES             Location/Qualifiers\n\
             \x20    CDS             100..900\n\
             ORIGIN      \n//\n",
        )
    }

    #[test]
    fn a_record_that_declares_a_length_and_ships_no_bases_is_not_zero_bp() {
        // `m.seq.len()` counts bases present, so a 5,386 bp plasmid whose file
        // carries no sequence was described as "0 bp circular" — the same class
        // of claim the GC field of that very line refuses to make. `pl info`
        // prints the declared length and says the bases are missing.
        let path = declared_but_absent();
        let text = call_tool("read_molecule", vec![("path", s(path.clone()))]);
        assert!(
            text.contains("5386 bp DECLARED, but this file carries no bases"),
            "{text}"
        );
        assert!(!text.starts_with("0 bp"), "{text}");
        // And a digest of nothing is not "no cuts": `pl digest` exits with this
        // sentence rather than answering.
        let (text, failed) = call_tool_full("digest", vec![("path", s(path))]);
        assert!(failed, "a digest of no bases succeeded: {text}");
        assert!(text.contains("no bases to digest"), "{text}");
    }

    #[test]
    fn a_standalone_annotation_track_is_not_described_as_zero_bp_linear() {
        // UGENE exports these: features, no ORIGIN, and no bp field or topology
        // word on the LOCUS line. It was reported as "0 bp linear", asserting
        // both a length the features contradict and a topology the file never
        // gave.
        let path = fixture(
            "track.gb",
            "LOCUS       Annotations                                             19-MAR-2018\n\
             FEATURES             Location/Qualifiers\n\
             \x20    CDS             242..1015\n\
             \x20    CDS             complement(1118..1951)\n//\n",
        );
        let text = call_tool("read_molecule", vec![("path", s(path))]);
        assert!(text.contains("annotation track"), "{text}");
        assert!(text.contains("1951 bp"), "{text}");
        assert!(
            !text.contains("linear"),
            "a topology nothing declared: {text}"
        );
    }

    #[test]
    fn an_argument_of_the_wrong_json_type_is_refused_rather_than_silently_defaulted() {
        // `as_i64().unwrap_or(30)` reads "absent" and "present but unreadable"
        // as the same thing, so `{"min_aa": "1000"}` — a number written as a
        // string, a routine tool-call artifact — silently became the default 30
        // and the caller was handed a 121 aa ORF for a 1,000 aa request.
        let path = fixture(
            "orf371.fa",
            &format!(">x\nATG{}TAAAAAAA\n", "GCC".repeat(120)),
        );
        for bad in [s("1000"), Value::Number(1000.5), Value::Bool(true)] {
            let (text, failed) = call_tool_full(
                "open_reading_frames",
                vec![("path", s(path.clone())), ("min_aa", bad.clone())],
            );
            assert!(failed, "min_aa {bad:?} was accepted: {text}");
            assert!(text.contains("min_aa must be a whole number"), "{text}");
        }
        let (text, failed) = call_tool_full(
            "open_reading_frames",
            vec![("path", s(path.clone())), ("table", s("2"))],
        );
        assert!(failed, "{text}");
        assert!(text.contains("table must be a whole number"), "{text}");
        // Omitted is still the default, and a real value still works.
        let text = call_tool("open_reading_frames", vec![("path", s(path.clone()))]);
        assert!(text.contains("min 30 aa"), "{text}");
        let text = call_tool(
            "open_reading_frames",
            vec![("path", s(path)), ("min_aa", Value::Number(1000.0))],
        );
        assert!(text.contains("no ORF of 1000 aa or more"), "{text}");
    }

    #[test]
    fn a_reply_that_lists_orfs_still_names_the_threshold_it_used() {
        // The threshold appeared only in the "no ORF of N aa or more" line,
        // which fires only when nothing was found — so the one case where a
        // substituted default could be caught was the one case it could not
        // reach. An assistant handed a list of ORFs had nothing to check the
        // request against.
        let path = fixture("orf.fa", ">x\nATGAAACCCGGGTAA\n");
        let text = call_tool(
            "open_reading_frames",
            vec![("path", s(path)), ("min_aa", Value::Number(2.0))],
        );
        assert!(text.contains("min 2 aa"), "{text}");
        assert!(text.contains(" aa, "), "an ORF was listed: {text}");
    }

    #[test]
    fn an_orf_that_laps_a_circular_molecule_reports_its_real_length() {
        // `start..end` is the ORF's extent only while `Orf::laps` is zero. On a
        // 19 bp circle a 33-base ORF reports start 5, end 18 — an inclusive
        // range of 14 bases, with start < end so nothing looks wrong — and the
        // line crossed the boundary reading as the ORF's full extent. A reader
        // who slices those coordinates gets 14 bases of the wrong sequence.
        let path = fixture(
            "tiny.gb",
            "LOCUS       tiny                    19 bp    DNA     circular SYN 01-JAN-2026\n\
             ORIGIN\n        1 CGTAATGCCTTTCCCTAAC\n//\n",
        );
        let text = call_tool(
            "open_reading_frames",
            vec![
                ("path", s(path)),
                ("table", Value::Number(1.0)),
                ("min_aa", Value::Number(1.0)),
            ],
        );
        let line = text
            .lines()
            .find(|l| l.starts_with("5..18"))
            .unwrap_or_else(|| panic!("no lapping ORF in {text:?}"));
        assert!(line.contains("10 aa"), "{line}");
        assert!(
            line.contains("33 bp"),
            "the range spans 14 bases and the ORF is 33: {line}"
        );
        assert!(
            line.contains("whole lap(s) short"),
            "nothing said the range is not the extent: {line}"
        );
    }

    #[test]
    fn an_orf_that_does_not_lap_says_nothing_about_laps() {
        // The control. A wrap note on an ORF that does not wrap would send a
        // reader looking for bases that are not there.
        let path = fixture("orf.fa", ">x\nATGAAACCCGGGTAA\n");
        let text = call_tool(
            "open_reading_frames",
            vec![("path", s(path)), ("min_aa", Value::Number(2.0))],
        );
        assert!(!text.contains("lap"), "{text}");
        assert!(text.contains("15 bp"), "4 aa and a stop: {text}");
    }
}
