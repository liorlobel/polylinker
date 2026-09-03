//! What the binary does when you run it.
//!
//! `bins/pl` had no integration tests at all, which is how a run that destroys
//! one of its own inputs, a checksum that silently describes record 1 of eight,
//! and an annealing temperature 50C off shipped together. Every defect below
//! was reproduced against the release binary before it was fixed, and every
//! test here was watched failing against the unfixed code — a test that passes
//! either way pins nothing.
//!
//! std only, no dev-dependencies: the same rule the correctness crates keep.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PL: &str = env!("CARGO_BIN_EXE_pl");

/// A GenBank record with bases, a feature and a declared topology.
fn genbank(name: &str, seq: &str, circular: bool) -> String {
    format!(
        "LOCUS       {name:<16}{} bp    DNA     {} SYN 26-JUL-2026
FEATURES             Location/Qualifiers
     misc_feature    1..5
                     /label=\"tag\"
ORIGIN
        1 {seq}
//
",
        seq.len(),
        if circular { "circular" } else { "linear" }
    )
}

/// A directory of our own. Left behind when a test fails, on purpose: the whole
/// point of several of these is what is on disk afterwards.
fn scratch(what: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pl-cli-{what}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(PL)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run pl")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

// --- finding 21: convert must not eat a file it has not read yet ------------

#[test]
fn converting_never_writes_over_a_file_that_is_still_an_input() {
    let dir = scratch("convert-victim");
    write(&dir, "seqA.fa", ">seqA\nAAAACCCCGGGGTTTT\n");
    let victim = genbank("seqA", "GGGGAAAACCCCTTTT", true);
    write(&dir, "seqA.gb", &victim);

    let out = run(&dir, &["convert", "seqA.fa", "seqA.gb", "--to", "genbank"]);

    // The point is not the exit code, it is the bytes: iteration 1 derived
    // `seqA.gb` from `seqA.fa` (locus_name strips the extension), found no
    // collision because the guard only compared against the input of that same
    // iteration, and wrote the FASTA-derived record over the user's GenBank
    // file. Iteration 2 then read back what iteration 1 had written, failed the
    // same-file guard, and the run ended with "nothing was overwritten".
    assert_eq!(
        std::fs::read_to_string(dir.join("seqA.gb")).unwrap(),
        victim,
        "seqA.gb was modified by a run that was converting seqA.fa"
    );
    assert!(!out.status.success(), "the collision must not be a success");
    assert!(
        stderr(&out).contains("seqA.gb"),
        "the user has to be told which file was in the way: {}",
        stderr(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn converting_two_files_that_do_not_collide_still_writes_both() {
    // The control: refusing a real collision must not cost the ordinary case.
    let dir = scratch("convert-ok");
    write(&dir, "one.fa", ">one\nAAAACCCCGGGGTTTT\n");
    write(&dir, "two.fa", ">two\nGGGGTTTTAAAACCCC\n");

    let out = run(&dir, &["convert", "one.fa", "two.fa", "--to", "genbank"]);

    assert!(out.status.success(), "{}", stderr(&out));
    assert!(dir.join("one.gb").is_file());
    assert!(dir.join("two.gb").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- finding 61: --stdout must not concatenate two documents ----------------

#[test]
fn two_inputs_are_never_concatenated_into_one_stdout_document() {
    let dir = scratch("stdout-concat");
    write(&dir, "a.gb", &genbank("a", "AAAACCCCGGGGTTTT", true));
    write(&dir, "b.gb", &genbank("b", "GGGGTTTTAAAACCCC", true));

    // Two SnapGene containers back to back parse without error -- read_blocks
    // validates the HEADER kind and magic for the first block only -- so b's
    // header is absorbed as a block of a's document and the last writer wins.
    let dna = run(
        &dir,
        &["convert", "a.gb", "b.gb", "--to", "dna", "--stdout"],
    );
    assert!(!dna.status.success());
    assert!(
        dna.stdout.is_empty(),
        "nothing may reach stdout once the run is refused"
    );

    // Two <svg> roots is not well-formed XML, and a second PDF leaves a trailing
    // xref pointing into the first document.
    let svg = run(&dir, &["export", "a.gb", "b.gb", "--stdout"]);
    assert!(!svg.status.success());
    assert!(svg.stdout.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_inputs_may_still_be_concatenated_as_fasta_because_fasta_is_a_stream() {
    // The control, and the reason this is not a blanket ban on multi-input
    // --stdout: FASTA and GenBank are record streams, and `>a` followed by `>b`
    // is exactly what a multi-FASTA is.
    let dir = scratch("stdout-fasta");
    write(&dir, "a.gb", &genbank("a", "AAAACCCCGGGGTTTT", true));
    write(&dir, "b.gb", &genbank("b", "GGGGTTTTAAAACCCC", true));

    let out = run(
        &dir,
        &["convert", "a.gb", "b.gb", "--to", "fasta", "--stdout"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out).matches('>').count(),
        2,
        "both records should be on the stream: {}",
        stdout(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- findings 24 and 58: a dropped record is never silent -------------------

#[test]
fn checksum_says_when_the_file_held_records_it_did_not_checksum() {
    let dir = scratch("checksum-multi");
    write(
        &dir,
        "assemblies.fa",
        ">contig1\nAAAACCCCGGGGTTTT\n>contig2\nGGGGTTTTAAAACCCC\n",
    );

    let out = run(&dir, &["checksum", "assemblies.fa"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    // An 8-contig Plasmidsaurus assembly used to print the file's basename and
    // one checksum pair from contig 1, shape-identical to a single-record file.
    assert!(
        text.contains("2 in this file"),
        "the record count belongs beside the checksum: {text}"
    );
    assert!(
        text.contains("only the first"),
        "and so does the scope of the claim: {text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn checksum_of_a_single_record_file_says_nothing_about_records() {
    // The control: the note must appear only when something was actually left
    // behind, or it becomes noise nobody reads.
    let dir = scratch("checksum-single");
    write(&dir, "one.fa", ">one\nAAAACCCCGGGGTTTT\n");

    let out = run(&dir, &["checksum", "one.fa"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(!text.contains("in this file"), "{text}");
    assert!(text.contains("lsseguid="), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn digest_says_that_only_the_first_record_of_the_file_was_digested() {
    let dir = scratch("digest-multi");
    write(
        &dir,
        "multi.fa",
        ">contig1\nGAATTCAAAACCCCGGGGTTTT\n>contig2\nGGATCCGGGGTTTTAAAACCCC\n",
    );

    let out = run(&dir, &["digest", "multi.fa", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // On stderr, so the JSON document on stdout is still one parseable answer.
    assert!(
        stderr(&out).contains("2 records in this file"),
        "'N unique cutter(s)' is a claim about the file: {}",
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("\"digests\""),
        "the answer itself is unchanged: {}",
        stdout(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn digest_of_a_single_record_file_warns_about_nothing() {
    let dir = scratch("digest-single");
    write(&dir, "one.fa", ">one\nGAATTCAAAACCCCGGGGTTTT\n");

    let out = run(&dir, &["digest", "one.fa", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !stderr(&out).contains("records in this file"),
        "{}",
        stderr(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- finding 59: the lowest Tm of a set with a hole in it is unknown --------

#[test]
fn a_failed_oligo_suppresses_the_advice_wherever_it_sits_in_the_list() {
    let dir = scratch("tm-order");
    // Middle position is the whole bug: `tms.clear()` in the Err arm was undone
    // by the success after it, so the advice was minimised over the survivors.
    let out = run(
        &dir,
        &["tm", "ATATATATATATATAT", "GGGGNGGGG", "GGGGGGCCGGGGCCGGGG"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("no annealing advice"),
        "the suppression has to be stated, not just performed: {text}"
    );
    assert!(
        !text.contains("annealing advice, from the lowest Tm"),
        "advice from a set with an unevaluated member is advice from the wrong number: {text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn advice_for_a_set_that_all_evaluated_comes_from_the_coldest_oligo() {
    // The control: the AT-rich 17.8C oligo, not the 69.2C one, sets the Ta.
    let dir = scratch("tm-ok");
    let out = run(&dir, &["tm", "ATATATATATATATAT", "GGGGGGCCGGGGCCGGGG"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("annealing advice, from the lowest Tm (17.8C)"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- finding 62: a mistyped option is not a default -------------------------

#[test]
fn a_mistyped_option_stops_the_run_instead_of_changing_the_answer() {
    let dir = scratch("unknown-flag");
    write(&dir, "p.gb", &genbank("p", "AAAACCCCGGGGTTTT", true));

    let out = run(&dir, &["orfs", "p.gb", "--min-a", "50"]);
    assert!(
        !out.status.success(),
        "--min-a left min_aa at 30 and printed every ORF the user excluded"
    );
    assert!(stderr(&out).contains("unknown option"), "{}", stderr(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_correctly_spelled_option_is_still_accepted() {
    let dir = scratch("known-flag");
    write(&dir, "p.gb", &genbank("p", "AAAACCCCGGGGTTTT", true));

    let out = run(&dir, &["orfs", "p.gb", "--min-aa", "50"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_after_the_verb_is_answered_rather_than_rejected() {
    // Rejecting unknown options must not turn a habit into an error.
    let dir = scratch("verb-help");
    let out = run(&dir, &["convert", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("CONVERT OPTIONS"), "{}", stdout(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

// --- finding 87: an unplaced read is not a base difference ------------------

#[test]
fn a_read_that_could_not_be_placed_is_not_counted_as_a_difference() {
    let dir = scratch("sanger-unplaced");
    let reference = "ACGGTTACCGATTGCAACGTTGCATCGGATCCAAGCTTGGCATGCTAGCA";
    let perfect = "ACGGTTACCGATTGCAACGTTGCATCGGA";

    let out = run(
        &dir,
        &[
            "sanger",
            "--ref-seq",
            reference,
            "--read",
            perfect,
            "--read",
            "",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("could not be placed"), "{text}");
    assert!(
        !text.contains("not dismissible"),
        "zero bases were compared for the unplaced read, so there is no difference to report: \
         {text}"
    );
    assert!(
        text.contains("1 read(s) could not be placed"),
        "and it still has to be counted somewhere: {text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_mismatch_is_still_counted_as_a_difference() {
    // The control: separating the counters must not lose genuine discrepancies.
    let dir = scratch("sanger-mismatch");
    let reference = "ACGGTTACCGATTGCAACGTTGCATCGGATCCAAGCTTGGCATGCTAGCA";
    let mutated = "ACGGTTACCGATTGCTACGTTGCATCGGA";

    let out = run(&dir, &["sanger", "--ref-seq", reference, "--read", mutated]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("1 difference(s) not dismissible"),
        "{}",
        stdout(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- finding 88: one unreadable file does not end `pl info` -----------------

#[test]
fn info_summarises_the_readable_files_even_when_an_earlier_one_is_missing() {
    let dir = scratch("info-missing");
    write(&dir, "good.gb", &genbank("good", "AAAACCCCGGGGTTTT", true));

    let out = run(&dir, &["info", "missing.fa", "good.gb"]);
    assert!(
        stdout(&out).contains("good.gb"),
        "USAGE promises to summarise each file: {}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("topology"), "{}", stdout(&out));
    assert!(stderr(&out).contains("missing.fa"), "{}", stderr(&out));
    // Unchanged, and deliberately so: xcheck_rust.py bails on a non-zero
    // status, and a read failure is a hard stop, not a soft mismatch.
    assert!(!out.status.success());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn info_json_still_reports_the_same_failure_the_same_way() {
    // The control on the branch that was already right: both branches now
    // continue past a read failure and both still exit non-zero.
    let dir = scratch("info-missing-json");
    write(&dir, "good.gb", &genbank("good", "AAAACCCCGGGGTTTT", true));

    let out = run(&dir, &["info", "--json", "missing.fa", "good.gb"]);
    let text = stdout(&out);
    assert!(text.contains("\"error\""), "{text}");
    assert!(text.contains("\"records_in_file\""), "{text}");
    assert!(!out.status.success());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- finding 77: both note notices have to survive a mutation ---------------

/// A `.dna` carrying the block 6 payload given, as bytes.
///
/// Hand-rolled rather than built with `pl_fileio::snapgene::write_blocks`,
/// because this file is std-only on purpose and the container is four lines:
/// each block is a kind byte, a big-endian `u32` length, and the payload.
fn dna(notes: &str) -> Vec<u8> {
    let mut header = b"SnapGene".to_vec();
    header.extend_from_slice(&1u16.to_be_bytes()); // DNA
    header.extend_from_slice(&15u16.to_be_bytes()); // export version
    header.extend_from_slice(&19u16.to_be_bytes()); // import version
    let mut out = Vec::new();
    for (kind, payload) in [
        (9u8, header),                             // HEADER
        (0u8, vec![0x01, b'A', b'C', b'G', b'T']), // SEQUENCE, circular
        (6u8, notes.as_bytes().to_vec()),          // NOTES
    ] {
        out.push(kind);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&payload);
    }
    out
}

fn write_bytes(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

/// A published citation lives one level below a note, and `Note` is flat.
const CITED: &str = r#"<Notes><Type>Synthetic</Type><References><Reference pubMedID="9335267" title="Precise deletions in E. coli"/></References></Notes>"#;

#[test]
fn info_names_the_part_of_the_notes_block_it_could_not_hold() {
    // Silencing this `println!` left `cargo test --workspace` green, in a repo
    // that already had a CLI harness. The notice *is* the fix for a loss that
    // cannot be undone, so it needs a test at the surface a user sees.
    let dir = scratch("info-notes");
    write_bytes(&dir, "cited.dna", &dna(CITED));

    let out = run(&dir, &["info", "cited.dna"]);
    let text = stdout(&out);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        text.contains("Notes/References/Reference"),
        "the path is the whole value of the notice: {text}"
    );
    assert!(
        text.contains("notes block this model cannot hold"),
        "and it must say what kind of thing was lost: {text}"
    );
    // Not folded into the locations line, which would have the CLI state
    // something false about coordinates.
    assert!(!text.contains("location(s)"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn converting_says_what_the_source_held_and_the_output_does_not() {
    // `pl convert --to dna` is the verb that destroys it: the citation is in the
    // input, absent from the output, and re-reading the output reports nothing
    // because by then there is nothing left to report. It bound the load report
    // five lines above the notice and read only `truncated()` from it, so this
    // ran with exit 0 and an empty stderr — which is, verbatim, what audit #77
    // opened by complaining about.
    let dir = scratch("convert-notes");
    write_bytes(&dir, "cited.dna", &dna(CITED));

    let out = run(
        &dir,
        &["convert", "cited.dna", "--to", "dna", "--outdir", "out"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("Notes/References/Reference"),
        "the loss happens here and has to be said here: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("the output does not carry"),
        "{}",
        stderr(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn converting_says_what_the_writer_could_not_carry() {
    // The other channel, and the one whose plain wrapper had thrown the report
    // away for as long as it existed: `<5UTC>` is an element name `xml::scan`
    // reads leniently and XML forbids, so the writer refuses it rather than
    // emitting a tag no parser can read. The legal sibling still goes out.
    let dir = scratch("convert-unwritable");
    write_bytes(
        &dir,
        "odd.dna",
        &dna("<Notes><5UTC>22:0:0</5UTC><Type>Synthetic</Type></Notes>"),
    );

    let out = run(
        &dir,
        &["convert", "odd.dna", "--to", "dna", "--outdir", "out"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("the dna writer could not carry"),
        "{}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("5UTC"), "{}", stderr(&out));

    let written = std::fs::read(dir.join("out").join("odd.dna")).unwrap();
    let text = String::from_utf8_lossy(&written);
    assert!(text.contains("<Type>Synthetic</Type>"), "{text}");
    assert!(!text.contains("5UTC"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ordinary_dna_converts_without_a_notice() {
    // The control. A notice that fires on every file is a notice nobody reads,
    // and both of the two above are one `is_empty()` from doing exactly that.
    let dir = scratch("convert-quiet");
    write_bytes(
        &dir,
        "plain.dna",
        &dna(r#"<Notes><Type>Synthetic</Type><Created UTC="22:0:0">2022.12.13</Created></Notes>"#),
    );

    let out = run(
        &dir,
        &["convert", "plain.dna", "--to", "dna", "--outdir", "out"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !stderr(&out).contains("could not carry") && !stderr(&out).contains("cannot hold"),
        "nothing was lost here: {}",
        stderr(&out)
    );
    // ...and the half of the timestamp that lives in the attribute is in the
    // file we wrote, which is finding #77 itself, measured through the CLI.
    let written = std::fs::read(dir.join("out").join("plain.dna")).unwrap();
    assert!(
        String::from_utf8_lossy(&written).contains(r#"<Created UTC="22:0:0">2022.12.13</Created>"#),
        "{}",
        String::from_utf8_lossy(&written)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// pl design --vector
// ---------------------------------------------------------------------------

/// A FASTA record, since `--vector` most often gets one and FASTA is the format
/// that carries no topology.
fn fasta(name: &str, seq: &str) -> String {
    format!(">{name}\n{seq}\n")
}

/// `n` deterministic pseudo-random bases carrying none of `absent`.
///
/// An LCG, matching `pl-design`'s own test fixtures, and **not** a repeated
/// motif: the first draft of these tests used `"ACCTTGCAAG".repeat(200)`, on
/// which every candidate primer occurs two hundred times, the off-target gate
/// refuses the whole design, and all three tests below failed with empty stdout
/// having proved nothing about vectors at all.
fn plain(n: usize, absent: &[&str]) -> String {
    let mut s: u64 = 20_260_729;
    let mut out = String::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        out.push(b"ACGT"[((s >> 24) & 3) as usize] as char);
    }
    // Break any occurrence of a site these tests place deliberately, so a
    // fixture cannot turn a control into a refusal.
    let flip = |c: u8| match c {
        b'A' => "C",
        b'C' => "A",
        b'G' => "T",
        _ => "G",
    };
    loop {
        let Some((i, m)) = absent
            .iter()
            .filter_map(|m| out.find(m).map(|i| (i, *m)))
            .min()
        else {
            return out;
        };
        let at = i + m.len() / 2;
        let with = flip(out.as_bytes()[at]);
        out.replace_range(at..at + 1, with);
    }
}

/// A methylation-blocked site in the vector is not a usable single cutter.
///
/// PROVEN TO FAIL against the shipped `--vector` block, which called
/// `pl_enzymes::cut_positions` and nothing else: both assertions fire, because
/// the Dam-blocked vector and the Dam-free control printed byte-identical
/// lines. Two source comments — `bins/pl/src/main.rs` and `pl_design::tail` —
/// said methylation *was* applied here. The bench consequence is a week: the
/// user linearises a dam+ prep, gets no cut, and the tool had told them that
/// site was where the insert goes.
#[test]
fn a_dam_blocked_site_in_the_vector_is_not_reported_as_the_one_to_use() {
    let dir = scratch("design-vector-meth");
    // ATCGAT out of the fixture so the only ClaI site anywhere is the one each
    // vector is built around; TCGA as well, so the flanks cannot spell one.
    let body = plain(2_000, &["ATCGAT", "TCGA"]);
    write(&dir, "template.fa", &fasta("t", &body));

    // ClaI's ATCGAT inside GATCGATC: Dam methylates the GATC that overlaps it
    // at each end, and the site is blocked.
    let blocked = format!("{}GATCGATC{}", &body[..1_000], &body[1_008..]);
    write(&dir, "blocked.fa", &fasta("v", &blocked));
    // The control: the same recognition site with no GATC overlapping it. AA
    // after the site and CC before it, so neither GATCGAT nor ATCGATC occurs.
    let free = format!("{}CCATCGATAA{}", &body[..1_000], &body[1_010..]);
    write(&dir, "free.fa", &fasta("v", &free));

    let args = |v: &'static str| {
        vec![
            "design",
            "template.fa",
            "--region",
            "400..1000",
            "--add-5",
            "ClaI",
            "--vector",
            v,
        ]
    };
    let dam = stdout(&run(&dir, &args("blocked.fa")));
    let none = stdout(&run(&dir, &args("free.fa")));

    assert!(
        dam.contains("blocked by Dam methylation"),
        "the blocked vector must say so:\n{dam}"
    );
    assert!(
        dam.contains("cuts 0 of them"),
        "a blocked site is not a cut:\n{dam}"
    );
    assert!(
        none.contains("cuts 1 of them -- which is what you want"),
        "the Dam-free control must still be usable:\n{none}"
    );
    assert!(
        !none.contains("methylation"),
        "and must not be warned about:\n{none}"
    );

    // --dam- is the escape hatch for a prep that already is dam-.
    let mut a = args("blocked.fa");
    a.push("--dam-");
    let off = stdout(&run(&dir, &a));
    assert!(
        off.contains("cuts 1 of them -- which is what you want"),
        "{off}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The vector's topology is stated, and a plasmid can be declared circular.
///
/// PROVEN TO FAIL: without `--vector-circular` and the topology on the line,
/// the first assertion fires — a 2,400 bp vector whose only EcoRI site
/// straddles base 1 was reported as "cuts 0 times -- so it cannot open this
/// vector", with nothing anywhere saying it had been read as linear.
#[test]
fn a_vector_site_across_the_origin_is_found_when_the_vector_is_a_plasmid() {
    let dir = scratch("design-vector-origin");
    let body = plain(2_000, &["GAATTC", "AATT"]);
    write(&dir, "template.fa", &fasta("t", &body));
    // GAA at the end, TTC at the start: one EcoRI site, only on a circle.
    let v = format!("TTC{}GAA", &body[3..1_997]);
    write(&dir, "v.fa", &fasta("v", &v));

    let base = vec![
        "design",
        "template.fa",
        "--region",
        "400..1000",
        "--add-5",
        "EcoRI",
        "--vector",
        "v.fa",
    ];
    let lin = stdout(&run(&dir, &base));
    assert!(
        lin.contains("read as LINEAR; pass --vector-circular"),
        "the assumption has to be disclosed:\n{lin}"
    );
    assert!(lin.contains("reads 0 sites"), "{lin}");

    let mut c = base.clone();
    c.push("--vector-circular");
    let circ = stdout(&run(&dir, &c));
    assert!(circ.contains("(2000 bp, circular)"), "{circ}");
    assert!(
        circ.contains("reads 1 site") && circ.contains("cuts 1 of them"),
        "{circ}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file with no bases is refused as a vector in the same words as a template.
///
/// PROVEN TO FAIL: with the two gates applied only to the template, as shipped,
/// this reports `EcoRI cuts anno.gb 0 times -- so it cannot open this vector` —
/// a verdict derived from zero bases, on which a user comparing candidate
/// backbones would eliminate one.
#[test]
fn an_annotation_only_vector_is_refused_rather_than_scored() {
    let dir = scratch("design-vector-anno");
    write(
        &dir,
        "template.fa",
        &fasta("t", &plain(2_000, &["GAATTC", "AATT"])),
    );
    write(
        &dir,
        "anno.gb",
        "LOCUS       anno                    4000 bp    DNA     circular SYN 26-JUL-2026\n\
         FEATURES             Location/Qualifiers\n\
         \x20    misc_feature    1..100\n\
         \x20                    /label=\"a\"\n//\n",
    );

    let out = run(
        &dir,
        &[
            "design",
            "template.fa",
            "--region",
            "400..1000",
            "--add-5",
            "EcoRI",
            "--vector",
            "anno.gb",
        ],
    );
    assert!(!out.status.success(), "{}", stdout(&out));
    let e = stderr(&out);
    assert!(e.contains("--vector anno.gb"), "{e}");
    assert!(
        e.contains("declares 4000 bases and carries none of them"),
        "the same words the template gets: {e}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The off-target scan's cost is stated **before** it is paid.
///
/// The scan is what this verb does that a designer deferring specificity to
/// BLAST cannot, and on a bacterial chromosome it is also the entire runtime:
/// measured, 3 s at 500 kb, 17 s at 1 Mb, 65 s at 2 Mb of random sequence and
/// about five and a half minutes on a real 9.1 Mb chromosome, with no bound and
/// no progress. Raising `--off-seed` would buy the time by making the scan
/// blind to exactly the second sites `pl_clone::pcr` objects to, so it is not
/// bought; what a user gets instead is the number and the knobs, up front. The
/// GUI refuses templates over 200 kb and sends people here, so here is where it
/// has to be said.
///
/// PROVEN TO FAIL: without the notice — the shipped behaviour — the first
/// assertion fires against empty stderr.
#[test]
fn a_template_big_enough_for_the_scan_to_dominate_says_so_first() {
    let dir = scratch("design-scan-notice");
    // Just over the 500 kb threshold. `--flank 0` keeps the enumeration to ten
    // oligos a side, so this pays for the notice rather than for the scan.
    let big = plain(520_000, &[]);
    write(&dir, "big.fa", &fasta("big", &big));
    let small = plain(20_000, &[]);
    write(&dir, "small.fa", &fasta("small", &small));

    let out = run(
        &dir,
        &["design", "big.fa", "--region", "9000..9800", "--flank", "0"],
    );
    let e = stderr(&out);
    assert!(
        e.contains("the off-target scan is the slow part here"),
        "the cost has to be stated before it is paid: {e:?}"
    );
    assert!(e.contains("--no-specificity"), "{e}");

    // Not said on a template where it is not true, or it is a notice nobody
    // reads; and not said when the scan is not going to run.
    let quiet = run(
        &dir,
        &[
            "design",
            "small.fa",
            "--region",
            "9000..9800",
            "--flank",
            "0",
        ],
    );
    assert!(
        !stderr(&quiet).contains("off-target scan is the slow part"),
        "{}",
        stderr(&quiet)
    );
    let skipped = run(
        &dir,
        &[
            "design",
            "big.fa",
            "--region",
            "9000..9800",
            "--flank",
            "0",
            "--no-specificity",
        ],
    );
    assert!(
        !stderr(&skipped).contains("off-target scan is the slow part"),
        "{}",
        stderr(&skipped)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// pl find-motif: the file's own topology
// ---------------------------------------------------------------------------

/// A 42 bp circle whose only EcoRI site spans bases 40,41,42,1,2,3.
///
/// `GAATTC` does not occur anywhere in it read as a line — checked by the
/// `--topology linear` leg below, which is also the override's own control.
const ORIGIN_SITE: &str = "TTCCGCGCGCGCGCGCGCGCGCGCGCGCGCGCGCGCGCGGAA";

/// A file that says it is a circle is searched as one.
///
/// PROVEN TO FAIL: `circular` came solely from `--topology`, whose default was
/// `linear` for a file as well as for `--seq`, so `mol.topology` was read into
/// nothing and this printed `ori.gb, 42 bp, linear` then `no hits` at exit 0 —
/// while `pl info`, `pl digest`, `pl primers` and `pl find` on the same bytes
/// all read it as a circle and all found the site. The `--json` leg is the one
/// that matters most: it emitted `"circular": false` and `"hits": []`, a
/// machine-readable false negative with no header text to notice.
#[test]
fn a_file_that_declares_a_circle_is_searched_as_a_circle() {
    let dir = scratch("find-motif-topology");
    write(&dir, "ori.gb", &genbank("ori", ORIGIN_SITE, true));

    let out = run(&dir, &["find-motif", "GAATTC", "ori.gb"]);
    let text = stdout(&out);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        text.contains("42 bp, circular"),
        "the header has to state the topology it actually used: {text}"
    );
    assert!(
        text.contains("wraps the origin") && text.contains("1 hit(s)"),
        "the site spans 40..3 and is only findable on a circle: {text}"
    );

    let json = stdout(&run(&dir, &["find-motif", "GAATTC", "ori.gb", "--json"]));
    assert!(json.contains("\"circular\": true"), "{json}");
    assert!(json.contains("\"wrapped\": true"), "{json}");

    // `--topology` still overrides, and is the control that the sequence really
    // does hold no linear occurrence.
    let lin = stdout(&run(
        &dir,
        &["find-motif", "GAATTC", "ori.gb", "--topology", "linear"],
    ));
    assert!(
        lin.contains("42 bp, linear") && lin.contains("no hits"),
        "{lin}"
    );

    // `--seq` carries no topology, so linear stays the default there.
    let bare = stdout(&run(&dir, &["find-motif", "GAATTC", "--seq", "TTCAAAAGAA"]));
    assert!(
        bare.contains("10 bp, linear") && bare.contains("no hits"),
        "{bare}"
    );
    let circ = stdout(&run(
        &dir,
        &[
            "find-motif",
            "GAATTC",
            "--seq",
            "TTCAAAAGAA",
            "--topology",
            "circular",
        ],
    ));
    assert!(circ.contains("1 hit(s)"), "{circ}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// pl annotate --genbank: the same four loss notices pl convert gives
// ---------------------------------------------------------------------------

/// A GenBank record holding two locations this reader cannot represent.
fn remote_locations() -> String {
    "LOCUS       remote                    42 bp    DNA     linear   SYN 26-JUL-2026\n\
     FEATURES             Location/Qualifiers\n\
     \x20    misc_feature    join(1..10,J00194.1:200..300)\n\
     \x20                    /label=\"split\"\n\
     \x20    misc_feature    gap(unk100)\n\
     \x20                    /label=\"ghost\"\n\
     ORIGIN\n\
     \x20       1 ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTAA\n\
     //\n"
        .to_string()
}

/// The verb that writes a record says what the record could not carry.
///
/// PROVEN TO FAIL: `cmd_annotate` read only `truncated()` off the load report
/// and called `genbank::write` rather than `write_reporting`, so
/// `pl annotate remote.gb --genbank` emitted a complete-looking record in which
/// "ghost" was gone entirely and "split" had become `misc_feature 1..10` — 10 bp
/// where the source claimed 111 — with **empty stderr** and exit 0.
/// `pl convert remote.gb --to genbank --stdout` writes a byte-identical record
/// to the same stream and reported the loss; only this verb was silent.
#[test]
fn annotating_to_genbank_says_what_the_output_could_not_carry() {
    let dir = scratch("annotate-genbank-loss");
    write(&dir, "remote.gb", &remote_locations());

    let out = run(&dir, &["annotate", "remote.gb", "--genbank"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let e = stderr(&out);
    assert!(
        e.contains("location(s) the reader could not represent"),
        "the loss happens here and has to be said here: {e:?}"
    );
    assert!(e.contains("gap(unk100)"), "{e}");
    assert!(e.contains("the output does not carry"), "{e}");
    // The record itself is still written — this is a notice, not a refusal.
    assert!(stdout(&out).contains("LOCUS"), "{}", stdout(&out));

    // The reader's other channel, on the format that has one: a nested citation
    // in a `.dna`'s notes block.
    write_bytes(&dir, "cited.dna", &dna(CITED));
    let cited = run(&dir, &["annotate", "cited.dna", "--genbank"]);
    assert!(
        stderr(&cited).contains("Notes/References/Reference"),
        "{}",
        stderr(&cited)
    );

    // The control: a notice that fires on every file is a notice nobody reads.
    write(
        &dir,
        "plain.gb",
        &genbank("plain", "AAAACCCCGGGGTTTT", true),
    );
    let quiet = run(&dir, &["annotate", "plain.gb", "--genbank"]);
    assert!(
        !stderr(&quiet).contains("could not carry")
            && !stderr(&quiet).contains("does not carry")
            && !stderr(&quiet).contains("cannot hold"),
        "nothing was lost here: {}",
        stderr(&quiet)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// pl design --vector: a multi-record file, and a vector nothing reads
// ---------------------------------------------------------------------------

/// Which record is the backbone is not a question record 1 can answer.
///
/// PROVEN TO FAIL: the vector's `LoadReport` was bound to `_` — the last such
/// binding in the binary — so a two-record file whose *second* record carries
/// the three EcoRI sites was judged on the first and the verdict printed as a
/// statement about the file: "EcoRI reads 0 sites in multi.fa (2000 bp, read as
/// LINEAR) and cuts 0 of them -- so it cannot open this vector", exit 0, empty
/// stderr, and the same sentence inside `--json`'s `warnings`, where a stderr
/// note could not have been seen at all. The user eliminates a good backbone.
#[test]
fn a_multi_record_vector_is_refused_rather_than_judged_on_record_one() {
    let dir = scratch("design-vector-multi");
    let body = plain(2_000, &["GAATTC", "AATT"]);
    write(&dir, "template.fa", &fasta("t", &body));
    // Record 2 carries three EcoRI sites; record 1 carries none.
    let cutter = format!(
        "{}GAATTC{}GAATTC{}GAATTC{}",
        &body[..300],
        &body[306..900],
        &body[906..1500],
        &body[1506..]
    );
    write(&dir, "only2.fa", &fasta("r2", &cutter));
    write(
        &dir,
        "multi.fa",
        &format!("{}{}", fasta("r1", &body), fasta("r2", &cutter)),
    );

    let args = |v: &'static str| {
        vec![
            "design",
            "template.fa",
            "--region",
            "400..1000",
            "--add-5",
            "EcoRI",
            "--vector",
            v,
            "--no-specificity",
        ]
    };
    let out = run(&dir, &args("multi.fa"));
    assert!(
        !out.status.success(),
        "a verdict about a backbone must not come from half the file:\n{}",
        stdout(&out)
    );
    let e = stderr(&out);
    assert!(e.contains("--vector multi.fa"), "{e}");
    assert!(e.contains("2 records"), "{e}");
    assert!(
        !stdout(&out).contains("cannot open this vector"),
        "and the wrong verdict must not be printed anyway:\n{}",
        stdout(&out)
    );

    // The control: the record it would have judged, alone, is still scored.
    let one = stdout(&run(&dir, &args("only2.fa")));
    assert!(
        one.contains("reads 3 sites in only2.fa"),
        "the single-record case must keep working:\n{one}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A vector nothing will read is refused, not accepted and ignored.
///
/// PROVEN TO FAIL: everything `--vector` produces comes out of a loop over
/// `[tail_five, tail_three]`, so with neither `--add-5` nor `--add-3` the file
/// was opened, parsed and put through both no-bases gates and then never
/// mentioned — `pl design template.fa --region 400..1000 --vector v.fa` printed
/// a full report in which the string "vector" did not occur once, exit 0, and
/// `--json`'s `warnings` said nothing either. Because the gates run first, an
/// unusable vector errored loudly while a usable one was completely silent, so
/// the silence read as a clean bill of health.
#[test]
fn a_vector_with_no_added_enzyme_is_refused_rather_than_ignored() {
    let dir = scratch("design-vector-inert");
    let body = plain(2_000, &["GAATTC", "AATT"]);
    write(&dir, "template.fa", &fasta("t", &body));
    write(&dir, "v.fa", &fasta("v", &body));

    let base = ["design", "template.fa", "--region", "400..1000"];
    let bad = run(
        &dir,
        &[&base[..], &["--vector", "v.fa", "--no-specificity"][..]].concat(),
    );
    assert!(!bad.status.success(), "{}", stdout(&bad));
    assert!(
        stderr(&bad).contains("--add-5") && stderr(&bad).contains("--vector"),
        "{}",
        stderr(&bad)
    );

    // `--spacer` is inert the same way, and independently of `--vector`.
    let sp = run(
        &dir,
        &[&base[..], &["--spacer", "AAAA", "--no-specificity"][..]].concat(),
    );
    assert!(!sp.status.success(), "{}", stdout(&sp));
    assert!(stderr(&sp).contains("--spacer"), "{}", stderr(&sp));

    // ...and the flags that only describe a vector need one to describe.
    let meth = run(
        &dir,
        &[&base[..], &["--dam-", "--no-specificity"][..]].concat(),
    );
    assert!(!meth.status.success(), "{}", stdout(&meth));
    assert!(stderr(&meth).contains("--dam-"), "{}", stderr(&meth));

    // The control: with a tail, the vector is read and reported as before.
    let ok = stdout(&run(
        &dir,
        &[
            &base[..],
            &["--add-5", "EcoRI", "--vector", "v.fa", "--no-specificity"][..],
        ]
        .concat(),
    ));
    assert!(ok.contains("reads 0 sites in v.fa"), "{ok}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Values, not just names: options that silently accepted anything
// ---------------------------------------------------------------------------

/// `--column` picks a width, so a value it does not understand is an error.
///
/// PROVEN TO FAIL: this was `a.get("column").map(|c| c == "double")`, a bare
/// `==` against one lowercase literal, so `--column Double` — with `--journal`
/// next door accepting `Nature` case-insensitively — silently selected the
/// single-column width. Measured: the EPS came out with `%%BoundingBox: 0 0 253
/// 253` (89 mm) instead of `0 0 519 519` (183 mm), and the journal type floor
/// then ran against the wrong width and printed "smallest type is 3.0 pt, below
/// nature's 5 pt minimum / ... or use --column double" — advising the user to do
/// exactly what they had typed.
#[test]
fn a_column_that_is_not_single_or_double_is_refused() {
    let dir = scratch("export-column");
    write(&dir, "map.gb", &genbank("map", "AAAACCCCGGGGTTTT", true));

    let width = |col: &str| -> String {
        stderr(&run(
            &dir,
            &[
                "export",
                "map.gb",
                "--journal",
                "nature",
                "--column",
                col,
                "--outdir",
                "out",
            ],
        ))
    };
    assert!(
        width("double").contains("183.0 x 183.0 mm"),
        "{}",
        width("double")
    );
    assert!(
        width("Double").contains("183.0 x 183.0 mm"),
        "--journal matches case-insensitively; --column has to as well: {}",
        width("Double")
    );
    assert!(
        width("single").contains("89.0 x 89.0 mm"),
        "{}",
        width("single")
    );

    for bad in ["doubel", "sngl", ""] {
        let out = run(
            &dir,
            &[
                "export",
                "map.gb",
                "--journal",
                "nature",
                "--column",
                bad,
                "--outdir",
                "out",
            ],
        );
        assert!(
            !out.status.success(),
            "--column {bad:?} silently became single-column: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains("expected single or double"),
            "{}",
            stderr(&out)
        );
    }

    // A `--column` nothing reads is no better than a mistyped one.
    let no_journal = run(
        &dir,
        &["export", "map.gb", "--column", "double", "--outdir", "out"],
    );
    assert!(!no_journal.status.success(), "{}", stderr(&no_journal));
    assert!(
        stderr(&no_journal).contains("--journal"),
        "{}",
        stderr(&no_journal)
    );
    let with_mm = run(
        &dir,
        &[
            "export", "map.gb", "--mm", "100", "--column", "double", "--outdir", "out",
        ],
    );
    assert!(!with_mm.status.success(), "{}", stderr(&with_mm));
    assert!(stderr(&with_mm).contains("--mm"), "{}", stderr(&with_mm));

    // The controls: neither option is broken on its own.
    let mm = run(
        &dir,
        &["export", "map.gb", "--mm", "100", "--outdir", "out"],
    );
    assert!(mm.status.success(), "{}", stderr(&mm));
    assert!(stderr(&mm).contains("100.0 x 100.0 mm"), "{}", stderr(&mm));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `--max-depth` that is not a number is refused, not ignored.
///
/// PROVEN TO FAIL: `.and_then(|v| v.parse().ok())` left `max_depth` at the
/// default 32, and since the only evidence a bound applied is the "scan
/// incomplete: ... deeper than --max-depth N" line, the run was
/// character-for-character identical to one with no `--max-depth` at all —
/// which is exactly what `parse_args`' own doc says must never happen.
#[test]
fn a_max_depth_that_is_not_a_number_is_refused() {
    let dir = scratch("index-max-depth");
    let root = dir.join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    write(&root, "a.gb", &genbank("a", "AAAACCCCGGGGTTTT", true));
    write(
        &root.join("sub"),
        "b.gb",
        &genbank("b", "GGGGTTTTAAAACCCC", true),
    );
    let idx = dir.join("idx");
    let idx = idx.to_str().unwrap();

    let bad = run(
        &dir,
        &[
            "index",
            "root",
            "--index-at",
            idx,
            "--max-depth",
            "not-a-number",
        ],
    );
    assert!(
        !bad.status.success(),
        "a bad depth walked the whole tree and said nothing: {}",
        stdout(&bad)
    );
    assert!(stderr(&bad).contains("--max-depth"), "{}", stderr(&bad));
    // `pl find` reaches the same parser.
    let find_bad = run(
        &dir,
        &[
            "find",
            "root",
            "--index-at",
            idx,
            "--motif",
            "ACGT",
            "--max-depth",
            "xyz",
        ],
    );
    assert!(!find_bad.status.success(), "{}", stdout(&find_bad));

    // The controls: a real depth still bounds the walk, and still says so.
    let zero = run(
        &dir,
        &["index", "root", "--index-at", idx, "--max-depth", "0"],
    );
    assert!(zero.status.success(), "{}", stderr(&zero));
    assert!(
        stderr(&zero).contains("deeper than --max-depth 0"),
        "{}",
        stderr(&zero)
    );
    assert!(stdout(&zero).contains("1 files"), "{}", stdout(&zero));
    let deep = run(
        &dir,
        &["index", "root", "--index-at", idx, "--max-depth", "8"],
    );
    assert!(stdout(&deep).contains("2 files"), "{}", stdout(&deep));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The unknown-enzyme message must not deny shipping enzymes that ship.
///
/// PROVEN TO FAIL: it read "not in the shipped table of 58 Type IIP enzymes. /
/// there is no BsaI, BsmBI, BbsI or SapI yet — use --motif GGTCTC to ask about
/// the site itself." Every clause was false at the time it was printed: all four
/// enzymes are in the table, 8 of the 58 entries are Type IIS, and `--enzyme
/// BsaI` hands `Motif::new` the same "GGTCTC" the suggested workaround does.
/// A user who mistypes any name is told the tool cannot do Golden Gate.
#[test]
fn the_unknown_enzyme_message_does_not_deny_the_type_iis_enzymes() {
    let dir = scratch("find-enzyme-message");
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    write(
        &root,
        "t.fa",
        ">t\nAAAAAAAAAAGGTCTCCGATCGGGGGGGGGGGAATTCTTTTTT\n",
    );
    let idx = dir.join("idx");
    let idx = idx.to_str().unwrap();

    let out = run(
        &dir,
        &["find", "root", "--index-at", idx, "--enzyme", "FooI"],
    );
    assert!(!out.status.success(), "{}", stdout(&out));
    let e = stderr(&out);
    assert!(
        !e.contains("there is no BsaI"),
        "the tool must not deny shipping an enzyme it ships: {e}"
    );
    assert!(
        !e.contains("Type IIP enzymes"),
        "8 of the 58 entries are Type IIS: {e}"
    );
    assert!(e.contains("Type IIS"), "{e}");

    // ...because the enzyme it denied resolves and searches.
    let bsai = run(
        &dir,
        &[
            "find",
            "root",
            "--index-at",
            idx,
            "--enzyme",
            "BsaI",
            "--json",
        ],
    );
    assert!(bsai.status.success(), "{}", stderr(&bsai));
    assert!(
        stdout(&bsai).contains("\"matched\": 1"),
        "{}",
        stdout(&bsai)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A query `pl find` cannot honour is refused, not dropped.
///
/// PROVEN TO FAIL: USAGE advertised `pl find <dir> [query] [filters]` and
/// `cmd_find` read only `files[0]`, so `pl find root GAATTC` and
/// `pl find root ZZZZZZ` — the second not even valid IUPAC — both listed every
/// record in the library and printed "1 record matched" at exit 0. A search
/// written the way `pl find-motif <IUPAC> <file>` is written returned the whole
/// library as its answer.
#[test]
fn a_positional_query_is_refused_rather_than_silently_dropped() {
    let dir = scratch("find-positional");
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    write(&root, "t.fa", ">t\nAAAAGAATTCAAAA\n");
    let idx = dir.join("idx");
    let idx = idx.to_str().unwrap();

    for query in ["GAATTC", "ZZZZZZ"] {
        let out = run(&dir, &["find", "root", "--index-at", idx, query]);
        assert!(
            !out.status.success(),
            "pl find root {query} answered with the whole library:\n{}",
            stdout(&out)
        );
        assert!(
            !stdout(&out).contains("1 record matched"),
            "and the unfiltered answer must not be printed anyway:\n{}",
            stdout(&out)
        );
        assert!(stderr(&out).contains("--motif"), "{}", stderr(&out));
    }

    // The controls: the folder alone still lists, and the named filter still
    // filters -- one of these motifs is present and the other is not.
    let all = run(&dir, &["find", "root", "--index-at", idx]);
    assert!(all.status.success(), "{}", stderr(&all));
    assert!(
        stdout(&all).contains("1 record matched"),
        "{}",
        stdout(&all)
    );
    let hit = run(
        &dir,
        &["find", "root", "--index-at", idx, "--motif", "GAATTC"],
    );
    assert!(hit.status.success(), "{}", stderr(&hit));
    assert!(
        stdout(&hit).contains("1 record matched"),
        "{}",
        stdout(&hit)
    );
    let miss = run(
        &dir,
        &["find", "root", "--index-at", idx, "--motif", "GGATCC"],
    );
    assert!(
        stdout(&miss).contains("0 records matched"),
        "{}",
        stdout(&miss)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Verdicts computed from nothing
// ---------------------------------------------------------------------------

/// A digest that recovered no overhang is not a clean bill of health.
///
/// PROVEN TO FAIL: `check(&[])` returns early on the empty slice, so a file with
/// no site for the enzyme printed "-> 1 fragment(s)", a blank line where the
/// overhang list goes, then "no structural fault found" at exit 0, and `--json`
/// gave `{"overhangs": [], "faults": [], "usable": true}`. The fragment count
/// does not disambiguate: a circle with one genuine BsaI junction prints the
/// same "-> 1 fragment(s)", and the `--json` path never emits it at all.
#[test]
fn a_digest_that_leaves_no_overhang_is_not_reported_as_usable() {
    let dir = scratch("goldengate-empty");
    // No GGTCTC and no GAGACC anywhere.
    write(
        &dir,
        "none.gb",
        &genbank("none", &"ACCTTGCAAG".repeat(30), true),
    );
    // One BsaI site, on a circle: one fragment, one real junction.
    write(
        &dir,
        "one.gb",
        &genbank(
            "one",
            &format!(
                "{}GGTCTCAATG{}",
                "ACCTTGCAAG".repeat(10),
                "ACCTTGCAAG".repeat(10)
            ),
            true,
        ),
    );

    let out = run(&dir, &["goldengate", "none.gb", "--enzyme", "BsaI"]);
    assert!(
        !out.status.success(),
        "an empty set passes every check by default, so it cannot be a pass:\n{}",
        stdout(&out)
    );
    assert!(
        !stdout(&out).contains("no structural fault found"),
        "{}",
        stdout(&out)
    );
    assert!(stderr(&out).contains("no overhang"), "{}", stderr(&out));

    // The `--json` consumer has no stderr to read, so exit code and the absence
    // of `"usable": true` are the whole signal.
    let json = run(
        &dir,
        &["goldengate", "none.gb", "--enzyme", "BsaI", "--json"],
    );
    assert!(!json.status.success(), "{}", stdout(&json));
    assert!(
        !stdout(&json).contains("\"usable\": true"),
        "{}",
        stdout(&json)
    );

    // The second route to the same empty set, and the one audit #42 opened
    // with: a linear part whose BsaI site sits so close to the end that the
    // overhang cannot form. #42's fix made `left_overhang` return an
    // empty-bases `Overhang` that `check` turns into `Fault::Incompatible`;
    // #43 then stopped `cut` producing that fragment at all, so the digest
    // yields no overhang and reaches `check` as an empty slice instead. The
    // library-level guard is unreachable from a file, which is why this one has
    // to be here.
    write(&dir, "short.fa", ">short\nAAAAAAAAAAAAGGTCTCAC\n");
    let short = run(
        &dir,
        &["goldengate", "short.fa", "--enzyme", "BsaI", "--json"],
    );
    assert!(
        !short.status.success(),
        "audit #42's own fixture still passed:\n{}",
        stdout(&short)
    );
    assert!(
        !stdout(&short).contains("\"usable\": true"),
        "{}",
        stdout(&short)
    );

    // The control, and the reason the fragment count cannot carry this on its
    // own: one genuine junction on a circle also prints "-> 1 fragment(s)".
    let ok = run(&dir, &["goldengate", "one.gb", "--enzyme", "BsaI"]);
    assert!(ok.status.success(), "{}", stderr(&ok));
    assert!(stdout(&ok).contains("-> 1 fragment(s)"), "{}", stdout(&ok));
    assert!(stdout(&ok).contains("ATGA"), "{}", stdout(&ok));
    let _ = std::fs::remove_dir_all(&dir);
}

/// No verb answers a question about a molecule that carries no bases.
///
/// PROVEN TO FAIL: on this record — 4000 bp declared, ORIGIN empty, which
/// `pl info` correctly describes as "4000 bp DECLARED, but this file carries no
/// bases" and which `pl digest` has always refused — `pl gel --cut EcoRI` said
/// "none of these enzymes cuts this molecule", `pl orfs` "no ORF of 30 aa or
/// more", `pl primers` "no binding site", `pl annotate` "nothing found",
/// `pl find-motif` "no hits" and `pl goldengate --enzyme BsaI` "no structural
/// fault found": six negative verdicts derived from zero bases, all at exit 0,
/// each also printing `0 bp` for a record that declares 4000. `pl find` over the
/// same file already excludes it from its coverage count and says why.
#[test]
fn no_verb_answers_about_a_file_that_declares_bases_and_carries_none() {
    let dir = scratch("no-bases-verdicts");
    write(
        &dir,
        "anno.gb",
        "LOCUS       anno                    4000 bp    DNA     circular SYN 26-JUL-2026\n\
         FEATURES             Location/Qualifiers\n\
         \x20    misc_feature    1..100\n\
         \x20                    /label=\"a\"\n//\n",
    );

    for args in [
        &["gel", "anno.gb", "--cut", "EcoRI"][..],
        &["orfs", "anno.gb"][..],
        &["primers", "anno.gb", "--primer", "GAATTCAAAACCCC"][..],
        &["annotate", "anno.gb"][..],
        &["find-motif", "GAATTC", "anno.gb"][..],
        &["goldengate", "anno.gb", "--enzyme", "BsaI"][..],
    ] {
        let out = run(&dir, args);
        assert!(
            !out.status.success(),
            "pl {} answered from zero bases:\n{}",
            args.join(" "),
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("carries none of them"),
            "pl {}: {}",
            args.join(" "),
            stderr(&out)
        );
        assert!(
            !stdout(&out).contains("0 bp"),
            "pl {} printed 0 bp for a record declaring 4000:\n{}",
            args.join(" "),
            stdout(&out)
        );
    }

    // The control: every one of them still answers about a molecule with bases.
    write(
        &dir,
        "real.gb",
        &genbank("real", "GAATTCAAAACCCCGGGGTTTTGAATTC", true),
    );
    for args in [
        &["gel", "real.gb", "--cut", "EcoRI"][..],
        &["orfs", "real.gb"][..],
        &["primers", "real.gb", "--primer", "GAATTCAAAACCCC"][..],
        &["annotate", "real.gb"][..],
        &["find-motif", "GAATTC", "real.gb"][..],
    ] {
        let out = run(&dir, args);
        assert!(
            out.status.success(),
            "pl {} must still answer: {}",
            args.join(" "),
            stderr(&out)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- 2026-07-29 workspace phase: the cross-area fixes ----------------------
//
// Each of these closes a `cross_area_requirements` entry that no per-area agent
// could reach, because the defect and its fix sat in different lanes. Every one
// was reproduced against the binary built from the per-area tree before being
// fixed, and the reproduction is quoted beside it.

#[test]
fn the_annealing_advice_applies_the_length_rule_it_prints() {
    // `pl_thermo::anneal` is length-blind, and `cmd_tm` folded its Tm list down
    // to a bare f64 one line before calling it -- so the +3 was applied at
    // every length while the note on that same printed line said "for primers
    // over 20 nt". Measured before the fix, on the 18 nt SP6 primer:
    // "Phusion  42C".
    let dir = scratch("tm-carveout");

    let out = run(&dir, &["tm", "ATTTAGGTGACACTATAG"]);
    let s = stdout(&out);
    assert!(
        s.contains("38.9C"),
        "the premise -- an 18 nt oligo at Tm 38.9C:\n{s}"
    );
    let phusion = s
        .lines()
        .find(|l| l.trim_start().starts_with("Phusion"))
        .unwrap_or_else(|| panic!("no Phusion line:\n{s}"))
        .to_string();
    assert!(
        phusion.contains("39C"),
        "an 18 nt primer is not 'over 20 nt', so its Ta is the Tm itself and \
         not Tm + 3 -- the rule this very line prints: {phusion}"
    );
    assert!(
        !phusion.contains("42C"),
        "42C is Tm + 3 applied to a primer the printed rule excludes: {phusion}"
    );

    // The control, and it is what stops the fix degenerating into "never add
    // three": a primer that IS over 20 nt still gets the offset.
    let out = run(&dir, &["tm", "ACGTACGTACGTAAGGCCTTACGT"]);
    let s = stdout(&out);
    assert!(s.contains("57.8C"), "the control's premise:\n{s}");
    let phusion = s
        .lines()
        .find(|l| l.trim_start().starts_with("Phusion"))
        .unwrap_or_else(|| panic!("no Phusion line:\n{s}"))
        .to_string();
    assert!(
        phusion.contains("61C"),
        "24 nt is over 20, so Tm + 3 applies: {phusion}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_enzyme_the_table_does_not_hold_is_named_even_beside_one_it_does() {
    // The guard was per CALL and not per NAME: `retain` followed by
    // `if results.is_empty()` fires only when EVERY name is unknown. Measured
    // before the fix, `--enzyme HaeIII --enzyme EcoRI` printed the EcoRI row,
    // exited 0 and dropped HaeIII without a word -- an answer about one enzyme
    // under the heading of two. HaeIII and DpnI are both absent from the 58-row
    // table and both ordinary things to ask for, so this is reachable by typing
    // a real enzyme name rather than by mistyping one.
    let dir = scratch("digest-per-name");
    write(&dir, "d.fa", ">d\nGAATTCGGATCCGGCCAAGCTTGATC\n");

    let out = run(
        &dir,
        &["digest", "d.fa", "--enzyme", "HaeIII", "--enzyme", "EcoRI"],
    );
    assert!(!out.status.success(), "must refuse:\n{}", stdout(&out));
    assert!(
        stderr(&out).contains("HaeIII"),
        "the missing name has to be the one named: {}",
        stderr(&out)
    );
    assert!(
        !stdout(&out).contains("EcoRI"),
        "and the half-answer must not be printed anyway:\n{}",
        stdout(&out)
    );

    // Every miss at once, so three wrong names do not cost three runs.
    let out = run(
        &dir,
        &["digest", "d.fa", "--enzyme", "DpnI", "--enzyme", "HaeIII"],
    );
    let e = stderr(&out);
    assert!(e.contains("DpnI") && e.contains("HaeIII"), "{e}");

    // Controls: a known name still digests, and a known NON-CUTTER still
    // reports no cuts rather than being confused with an unknown name.
    let out = run(&dir, &["digest", "d.fa", "--enzyme", "EcoRI"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("EcoRI"), "{}", stdout(&out));
    let out = run(&dir, &["digest", "d.fa", "--enzyme", "NotI"]);
    assert!(
        out.status.success(),
        "a real enzyme that does not cut is not an error: {}",
        stderr(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_orf_that_laps_the_molecule_reports_its_real_length() {
    // `start..end` is an inclusive span round the circle, and it stops being
    // the ORF's extent the moment `laps > 0`. Measured before the fix on this
    // 19 bp circle: "5..18  10 aa", a range of 14 bases for an ORF of 33, with
    // `start < end` so not even the wrap was visible -- while `--translate`
    // printed all ten residues, because `bases_of` already used `o.bases()`.
    // One line disagreeing with itself.
    let dir = scratch("orf-laps");

    let out = run(
        &dir,
        &[
            "orfs",
            "--seq",
            "CGTAATGCCTTTCCCTAAC",
            "--circular",
            "--table",
            "1",
            "--min-aa",
            "1",
            "--json",
        ],
    );
    let s = stdout(&out);
    assert!(
        s.contains("\"laps\": 1"),
        "the lap count must cross the boundary:\n{s}"
    );
    assert!(
        s.contains("\"bp\": 33"),
        "10 aa plus a stop is 33 bases:\n{s}"
    );
    assert!(
        s.contains("\"start\": 5") && s.contains("\"end\": 18"),
        "the coordinates themselves are unchanged:\n{s}"
    );

    let out = run(
        &dir,
        &[
            "orfs",
            "--seq",
            "CGTAATGCCTTTCCCTAAC",
            "--circular",
            "--table",
            "1",
            "--min-aa",
            "1",
        ],
    );
    let s = stdout(&out);
    assert!(
        s.contains("33 bp in all"),
        "the text form says it too:\n{s}"
    );

    // The control: an ORF that does not lap says nothing about laps, or the
    // note appears on every ORF and therefore means nothing.
    let out = run(
        &dir,
        &[
            "orfs",
            "--seq",
            "ATGCCCTTTTAA",
            "--table",
            "1",
            "--min-aa",
            "1",
            "--json",
        ],
    );
    assert!(stdout(&out).contains("\"laps\": 0"), "{}", stdout(&out));
    let out = run(
        &dir,
        &[
            "orfs",
            "--seq",
            "ATGCCCTTTTAA",
            "--table",
            "1",
            "--min-aa",
            "1",
        ],
    );
    assert!(
        !stdout(&out).contains("laps the molecule"),
        "{}",
        stdout(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_disagrees_with_itself_says_so_rather_than_being_answered_about() {
    // `Molecule::validate()` was reached from exactly one place in the whole
    // workspace -- pl-gui's document loader -- so every check it performs was
    // invisible from the terminal. The reachable case is dull: a GenBank record
    // whose `//` is missing and which has something after the sequence. The
    // ORIGIN loop reads on to end of file, so this record loads as 30 bases
    // against a LOCUS line that says 12, and `pl info` printed both numbers
    // without ever remarking that they disagree.
    let dir = scratch("self-contradiction");
    write(
        &dir,
        "foot.gb",
        "LOCUS       foot                      12 bp    DNA     linear   SYN 01-JAN-2026\n\
         ORIGIN\n        1 acgtacgtacgt\n--\nSent from my iPhone\n",
    );

    let out = run(&dir, &["info", "foot.gb"]);
    assert!(
        stderr(&out).contains("declares 12 bases but carries 30"),
        "the contradiction has to be named: {}",
        stderr(&out)
    );
    // A notice, not a refusal: the bases may well be the ones the user wants.
    assert!(out.status.success(), "{}", stderr(&out));

    // The control, and it is the one that matters -- a notice that fires on
    // every file is a notice nobody reads.
    write(&dir, "ok.gb", &genbank("ok", "ACGTACGTACGTACGT", false));
    let out = run(&dir, &["info", "ok.gb"]);
    assert!(
        !stderr(&out).contains("declares"),
        "an ordinary file must say nothing: {}",
        stderr(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_origin_crossing_feature_reports_the_wrap_and_not_the_whole_molecule() {
    // `Feature::start`/`end` are a min and a max over the segments, and
    // `genbank::write` emits an origin-crossing feature as `join(37..40,1..7)`
    // -- so the min start is always exactly 1 and the max end always exactly
    // the molecule length. Measured before the fix, an 11 bp promoter came out
    // of a shipped machine-readable format as `"start": 1, "end": 40`, spanning
    // the whole molecule, with only `"segments": 2` beside it and the true
    // coordinates unrecoverable from the document.
    let dir = scratch("extent-json");
    write(
        &dir,
        "wrap.gb",
        "LOCUS       wrap                      40 bp    DNA     circular SYN 01-JAN-2026\n\
         FEATURES             Location/Qualifiers\n\
         \x20    promoter        join(37..40,1..7)\n\
         \x20                    /label=\"wrapped\"\n\
         ORIGIN\n        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt\n//\n",
    );

    let out = run(&dir, &["info", "wrap.gb", "--json"]);
    let s = stdout(&out);
    assert!(
        s.contains("\"start\": 37") && s.contains("\"end\": 7"),
        "an origin-crossing feature is reported as the wrap it is, the way \
         Molecule::subseq reads a pair:\n{s}"
    );
    assert!(
        !s.contains("\"start\": 1, \"end\": 40"),
        "and not as the whole molecule:\n{s}"
    );

    // The control: an ordinary spliced join really does run from its lowest
    // coordinate to its highest, and must not be turned into a wrap.
    write(
        &dir,
        "spliced.gb",
        "LOCUS       spliced                   40 bp    DNA     circular SYN 01-JAN-2026\n\
         FEATURES             Location/Qualifiers\n\
         \x20    CDS             join(5..10,20..30)\n\
         \x20                    /label=\"exons\"\n\
         ORIGIN\n        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt\n//\n",
    );
    let out = run(&dir, &["info", "spliced.gb", "--json"]);
    let s = stdout(&out);
    assert!(
        s.contains("\"start\": 5") && s.contains("\"end\": 30"),
        "a spliced join keeps its min/max:\n{s}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the exported figure: what it is called, and what is on it -------------

/// A circular plasmid with exactly one EcoRI site and nothing else notable.
///
/// `ACGT` repeated never spells `GAATTC`, so the one copy inserted here is the
/// only one and EcoRI is a unique cutter.
fn one_site_plasmid() -> String {
    let mut seq = "ACGT".repeat(100);
    seq.push_str("GAATTC");
    seq.push_str(&"ACGT".repeat(100));
    genbank("pKoVtest", &seq, true)
}

/// A 1,119 bp circular plasmid with one unique, one dual and one multi cutter.
///
/// `--sites` has existed since e087e27 and no test had ever handed it `dual` or
/// `all` on a molecule with a non-unique cutter, which is why the mention-counting
/// bug survived a corpus that already contained the guard against it. Verified
/// with `pl digest`: EXACTLY three cutters — XhoI 1 cut at 996, BamHI 2 at
/// 746/871, EcoRI 5 at 121/246/371/496/621 — and nothing else in the 58-enzyme
/// table touches it. `GATTACA` holds no palindromic 6-mer, so the filler
/// introduces nothing; five copies of the same site is what makes EcoRI a multi
/// cutter and therefore what makes the arithmetic visible.
///
/// 1,119 bp at the default 720 pt canvas drops nothing and shortens nothing, so a
/// test over this fixture is entirely layout-independent: it fails on the unit and
/// only on the unit.
fn multi_cutter_plasmid() -> String {
    let f = "GATTACA".repeat(17);
    let mut seq = String::new();
    for _ in 0..5 {
        seq.push_str(&f);
        seq.push_str("GAATTC"); // EcoRI, 5 cuts
    }
    for _ in 0..2 {
        seq.push_str(&f);
        seq.push_str("GGATCC"); // BamHI, 2 cuts
    }
    seq.push_str(&f);
    seq.push_str("CTCGAG"); // XhoI, 1 cut
    seq.push_str(&f);
    genbank("pMULTI", &seq, true)
}

/// PROVEN TO FAIL at e087e27, on both assertions.
///
/// `pl_draw::scene` fell through to the literal string `"unnamed"` when the
/// molecule had no name, and nothing passed a filename in for it to say anything
/// else — so the centre caption and the SVG `<title>` of every figure ever
/// exported from a `.dna` read `unnamed`, while the map on screen said
/// "pKoV with His decR.dna". The `.dna` container carries no molecule name at
/// all, so this is every SnapGene file.
///
/// And `crates/pl-draw` held no reference to an enzyme anywhere, so the figure
/// carried no restriction sites either: a user reads the cutters off the map to
/// plan a digest, exports the figure, and gets a picture with nothing to plan
/// from.
#[test]
fn an_exported_figure_is_named_after_the_file_and_carries_its_cut_sites() {
    let dir = scratch("export-title");
    write(&dir, "pKoV with His decR.gb", &one_site_plasmid());
    let c = run(&dir, &["convert", "pKoV with His decR.gb", "--to", "dna"]);
    assert!(c.status.success(), "{}", stderr(&c));
    // `convert` names its output through `locus_name` on the INPUT filename,
    // which sanitises every non-alphanumeric and truncates to 16 — right for a
    // filename and exactly the function a caption must not go through, since it
    // answers `pKoV_with_His_de` for this file. Put the output back under the
    // name a person would give it.
    std::fs::rename(
        dir.join("pKoV_with_His_de.dna"),
        dir.join("pKoV with His decR.dna"),
    )
    .unwrap_or_else(|e| panic!("{e}: {}", stdout(&c)));

    let o = run(&dir, &["export", "pKoV with His decR.dna", "--stdout"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let svg = stdout(&o);
    assert!(
        !svg.contains("unnamed"),
        "the figure that goes into a paper is captioned `unnamed`"
    );
    assert!(
        svg.contains("<title>pKoV with His decR</title>"),
        "the caption is the file's name without its container: {}",
        svg.lines().take(6).collect::<Vec<_>>().join(" ")
    );
    assert!(
        svg.contains("EcoRI"),
        "the one unique cutter is nowhere on the exported map"
    );
}

/// The control, and it PASSES at e087e27: a real molecule name wins.
///
/// Paired with the test above on purpose. The fix adds a fallback and must not
/// touch the case where the file said what the molecule is called — a GenBank
/// LOCUS name is a real name and a filename is a guess.
#[test]
fn a_locus_name_still_beats_the_filename_it_was_saved_under() {
    let dir = scratch("export-locus");
    write(&dir, "some other name.gb", &one_site_plasmid());
    let o = run(&dir, &["export", "some other name.gb", "--stdout"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let svg = stdout(&o);
    assert!(svg.contains("<title>pKoVtest</title>"), "{svg:.400}");
    assert!(!svg.contains("some other name"));
}

/// PROVEN TO FAIL at e087e27 — there is no `--sites` there, so the run is
/// refused with "unknown option".
///
/// Refused positively rather than ignored, the way `--column`, `--topology` and
/// `--to` all are: a mistyped filter that silently means something else is how a
/// user comes to believe a site is absent.
#[test]
fn the_site_filter_is_stated_and_a_typo_is_refused() {
    let dir = scratch("export-sites");
    write(&dir, "map.gb", &one_site_plasmid());

    let none = run(&dir, &["export", "map.gb", "--sites", "none", "--stdout"]);
    assert!(none.status.success(), "{}", stderr(&none));
    assert!(
        !stdout(&none).contains("EcoRI"),
        "--sites none asked for no sites"
    );
    // Drawing nothing is not licence to say anything. This plasmid has exactly one
    // UNIQUE cutter, EcoRI at 402, and at 0ebaa41 `--sites none` described it as a
    // MULTI cutter — `0 of 3 cutters labelled · 0 dual, 3 multi not drawn` — in the
    // figure and on stderr, because `Sites::of` had no bucket for a single cutter
    // the filter turned away. It closed, so nothing caught it.
    //
    // The three cutters are `pl digest`'s own answer for this sequence and not an
    // assumption about it: EcoRI once at 402, and BsiWI 199 times and SnaBI 198
    // times, because `CGTACG` and `TACGTA` both fall out of an `ACGT` repeat. The
    // comment here used to claim one cutter, and the number it asserted was wrong
    // in the same direction — a fixture believed rather than measured.
    assert!(
        stderr(&none).contains("0 of 3 cutters labelled · 1 single, 0 dual, 2 multi not drawn"),
        "--sites none misdescribes a lone single cutter: {}",
        stderr(&none)
    );

    // `dual` admits the single cutters as well: an excision wants a pair of
    // sites and one of them is often the only copy of its enzyme.
    let dual = run(&dir, &["export", "map.gb", "--sites", "dual", "--stdout"]);
    assert!(dual.status.success(), "{}", stderr(&dual));
    assert!(stdout(&dual).contains("EcoRI"));

    let typo = run(&dir, &["export", "map.gb", "--sites", "uniqe", "--stdout"]);
    assert!(!typo.status.success(), "a typo must not be ignored");
    assert!(
        stderr(&typo).contains("unique") && stderr(&typo).contains("dual"),
        "and it must say what is allowed: {}",
        stderr(&typo)
    );
}

/// PROVEN TO FAIL at e087e27 and against the working tree as handed over: the
/// figure never said what it was not showing, and neither did the command.
///
/// `--sites unique` is the DEFAULT, so `pl export` on an ordinary plasmid drops
/// every dual and multi cutter — 18 of 40 on the user's own file — with nothing
/// in the SVG, nothing in the PDF and nothing on stderr. The desktop map has said
/// it since the L-ring landed; `docs/PLAN.md` item 33 calls a silent filter "the
/// one documented case of this software category costing a user a month of bench
/// time", and of the two artefacts the figure is the one that leaves the machine
/// and reaches a reader with no Enzymes tab to check it against.
///
/// The arithmetic has to close as well as be present. On pET28a the on-screen
/// line read `14 of 31 cutters labelled · 7 dual, 1 multi not drawn` — 14 + 7 + 1
/// against a stated 31 — because it counted LABELS, and a folded tick names
/// several enzymes.
#[test]
fn the_figure_and_the_command_both_say_which_cutters_were_left_out() {
    let dir = scratch("export-disclosure");
    write(&dir, "map.gb", &one_site_plasmid());

    let o = run(&dir, &["export", "map.gb", "--stdout"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let (svg, err) = (stdout(&o), stderr(&o));

    assert!(
        svg.contains("cutters"),
        "the exported figure does not state its own filter: {svg:.600}"
    );
    assert!(
        err.contains("cutters labelled"),
        "the command does not state it either: {err}"
    );

    // `<labelled> of <cutters>` plus the dual and multi it says are not drawn is
    // exactly `<cutters>`, or the line tells the reader enzymes went missing that
    // did not.
    let line = err
        .lines()
        .find(|l| l.contains("cutters labelled"))
        .expect("the line is there");
    let nums: Vec<u32> = line
        .rsplit(':')
        .next()
        .unwrap_or(line)
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .collect();
    assert!(nums.len() >= 4, "{line}");
    let (labelled, cutters, dual, multi) = (nums[0], nums[1], nums[2], nums[3]);
    assert_eq!(
        labelled + dual + multi,
        cutters,
        "{line} does not account for every cutter"
    );
    assert!(labelled >= 1, "{line}: EcoRI cuts once and is on the map");
}

/// PROVEN TO FAIL at 0ebaa41 by PROCESS EXIT CODE, before any parsing.
///
/// `CARGO_BIN_EXE_pl` is the DEBUG binary, so `debug_assert!(d.closes(), ..)` in
/// `cmd_export` is live under `cargo test`. At 0ebaa41 this fixture aborts:
///
///   `--sites dual` -> exit 101, `Disclosure { cutters: 3, labelled: 3, dual: 0,
///                     multi: 1, hidden: 0, shortened: 0 }`
///   `--sites all`  -> exit 101, `labelled: 8` against `cutters: 3`
///
/// So the guard was coded, correct and already firing; nothing needed building to
/// catch it. What was missing was a molecule the guard could bite on: the sibling
/// above runs at the DEFAULT filter on a one-EcoRI plasmid, where a mention, a
/// label and an enzyme are the same integer.
///
/// Four things the sibling does not do, each of which caught a distinct part of
/// this defect:
///  * it exercises `--sites dual` and `--sites all`, the two modes whose counts
///    were wrong;
///  * it includes `hidden` in the sum, which the sibling drops — a canvas that
///    dropped labels escaped it entirely;
///  * it asserts `labelled <= cutters`, which is the "8 of 3" and "71 of 40"
///    shape a reader notices and no arithmetic identity forbids;
///  * it exercises `--sites none`, whose BUCKETS were wrong while its sum closed.
///    That row is why `closes()` alone is not the whole guard: it passed on
///    `0 of 3 · 1 dual, 2 multi` for a fixture with one single, one dual and one
///    multi cutter. Pinning the exact sentence is what sees a misclassification,
///    and pinning it here rather than in `pl-draw` is deliberate — `pl-draw`
///    cannot depend on `pl-enzymes`, so it has no way to know that XhoI cuts once.
#[test]
fn the_disclosure_closes_on_a_multi_cutter_in_every_sites_mode() {
    let dir = scratch("export-disclosure-multi");
    write(&dir, "multi.gb", &multi_cutter_plasmid());

    // XhoI is unique, BamHI dual, EcoRI multi — so what each filter admits and
    // what it must own up to excluding is fully determined by the fixture.
    for (mode, want) in [
        (
            "unique",
            "1 of 3 cutters labelled · 1 dual, 1 multi not drawn",
        ),
        (
            "dual",
            "2 of 3 cutters labelled · 0 dual, 1 multi not drawn",
        ),
        ("all", "3 of 3 cutters labelled"),
        // The fourth mode, and the one that was still wrong after the counting
        // fix. `Sites::of` read `if keep {..} else if n == 2 { dual } else
        // { multi }`, so under `--sites none` — where nothing is kept — XhoI's
        // single cut fell into `multi` and this line said `0 of 3 cutters
        // labelled · 1 dual, 2 multi not drawn`. It CLOSES (0 + 0 + 1 + 2 == 3),
        // which is why ten plasmids, four widths and `debug_assert!(d.closes())`
        // all passed over it: the sum was right and the classes were not. On the
        // user's pKoV the same arm printed `0 of 40 cutters labelled · 12 dual,
        // 28 multi not drawn` into the SVG and the EPS, telling a reader planning
        // a digest that a plasmid with 22 unique cutters has none of them.
        (
            "none",
            "0 of 3 cutters labelled · 1 single, 1 dual, 1 multi not drawn",
        ),
    ] {
        let o = run(&dir, &["export", "multi.gb", "--sites", mode, "--stdout"]);
        // Before the counts: the run has to finish. At 0ebaa41 two of these four
        // abort here.
        assert!(
            o.status.success(),
            "--sites {mode} did not complete: {}",
            stderr(&o)
        );
        let err = stderr(&o);
        let line = err
            .lines()
            .find(|l| l.contains("cutters labelled"))
            .unwrap_or_else(|| panic!("--sites {mode} said nothing about cutters: {err}"));
        assert!(
            line.ends_with(want),
            "--sites {mode}\n  said {line:?}\n  want ...{want:?}"
        );

        // And the arithmetic, parsed back out of the sentence a reader sees —
        // `hidden` included, which is the clause the sibling throws away.
        let tail = line.rsplit(':').next().unwrap_or(line);
        let nums: Vec<u32> = tail
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().unwrap())
            .collect();
        assert!(nums.len() >= 2, "{line}");
        let (labelled, cutters) = (nums[0], nums[1]);
        // Every exclusion bucket is read BY NAME and not by position. `shortened`
        // is deliberately a LABEL count that `closes()` excludes, so "the fifth
        // number" would fold a label count into an enzyme sum the moment a
        // narrower canvas said "· 3 shortened" — and position broke outright when
        // `single` was added, silently reading the new bucket as `dual`.
        //
        // Over `tail` and not `line`: the fixture is called `multi.gb`, so
        // `line.find("multi")` lands in the filename and reads the wrong number.
        let before = |what: &str| -> u32 {
            tail.find(what)
                .and_then(|i| {
                    tail[..i]
                        .rsplit(|c: char| !c.is_ascii_digit())
                        .find(|s| !s.is_empty())?
                        .parse()
                        .ok()
                })
                .unwrap_or(0)
        };
        let hidden = before("would not fit");
        let single = before("single");
        let dual = before("dual");
        let multi = before("multi");
        assert_eq!(
            labelled + hidden + single + dual + multi,
            cutters,
            "--sites {mode}: {line} does not account for every cutter"
        );
        assert!(
            labelled <= cutters,
            "--sites {mode}: {line} labels more enzymes than cut the molecule"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// --- A1: the canvas nothing bounds -----------------------------------------

/// A raster nobody's machine can hold is refused, with the arithmetic in the
/// message.
///
/// PROVEN TO FAIL against the working tree of 2026-08-04. `--dpi` is banded
/// 72..=2400 (`bins/pl/src/main.rs`), `--mm` 5..=500 and `--width`/`--height`
/// 16..=20000, each on its own, and nothing looked at the product. The
/// invocation below is the one the dpi band's own comment names as the reason
/// 2400 is the ceiling, and at 183 mm it is 17,291 px square: 298,978,681
/// pixels, and a measured 10.99 GB of live heap (`36.75` bytes per pixel —
/// `Image::filled`'s 3, the filtered scanlines' 3, `deflate::lz77`'s
/// `prev = vec![usize::MAX; n]` at 24 and its `Sym` reservation at 6). On a
/// machine that cannot serve that, `handle_alloc_error` aborts: no diagnostic,
/// no partial file, no mention of the dpi that caused it. On a machine that
/// can, it takes minutes and produces a figure no journal asked for.
///
/// Observed failure before the fix, on a 128 GB machine where the allocation
/// succeeds: `exit status was success` — the export ran to completion and
/// wrote `map.png`.
#[test]
fn a_raster_too_big_to_hold_is_refused_before_it_is_allocated() {
    let dir = scratch("export-png-budget");
    write(&dir, "map.gb", &genbank("map", "AAAACCCCGGGGTTTT", true));

    let out = run(
        &dir,
        &[
            "export",
            "map.gb",
            "--png",
            "--journal",
            "nature",
            "--column",
            "double",
            "--dpi",
            "2400",
        ],
    );
    let err = stderr(&out);
    assert!(
        !out.status.success(),
        "a 299-megapixel export was attempted rather than refused: {err}"
    );
    assert!(
        !dir.join("map.png").exists(),
        "the refusal still left a file behind"
    );
    // Everything the user needs to act: what they asked for, what it came to,
    // what the limit is, and a resolution that fits. A message missing the last
    // one leaves them guessing at dpi values until one works.
    for want in ["2400", "17291", "100", "1388"] {
        assert!(
            err.contains(want),
            "the refusal does not mention {want:?}: {err}"
        );
    }

    // The control: the same figure at the default resolution still exports.
    // A guard that refuses a 4.7-megapixel publication figure is a worse bug
    // than the one it fixes.
    let ok = run(
        &dir,
        &[
            "export",
            "map.gb",
            "--png",
            "--journal",
            "nature",
            "--column",
            "double",
        ],
    );
    assert!(ok.status.success(), "{}", stderr(&ok));
    assert!(dir.join("map.png").is_file(), "{}", stderr(&ok));
    let _ = std::fs::remove_dir_all(&dir);
}

// --- A4: the size on the status line is the size in the file ---------------

/// `pl export --png` reports the dimensions `IHDR` actually holds.
///
/// PROVEN TO FAIL against the working tree of 2026-08-04, where the status
/// line came from `page::Fit::pixels` and the canvas came from
/// `raster::size(sc, png_scale(..))`. Those are two roundings, not one: `Fit`
/// rounds each axis against the printed size independently, while the canvas
/// derives its scale from the *already rounded* width and rounds the height
/// against that. Observed failure, verbatim:
///
/// ```text
/// pl printed 1134 x 850 px and IHDR says 1134 x 851
/// ```
///
/// Swept over five aspect ratios (4:3, 16:9, 3:4, ~golden, 2:5) x `--mm`
/// 20..=200 x `--dpi` {72, 150, 300, 600}, **1,063 of 3,620 combinations
/// disagree** — 656 with the file taller than reported and 407 shorter. So
/// this is the ordinary case, not a corner: a user sizing a figure to a
/// journal's pixel requirement is told a number the file does not carry.
///
/// Non-square on purpose. The GUI's figure options are 720 x 720, where the
/// two roundings coincide for every mm and dpi, which is why the existing
/// GUI-vs-CLI byte-parity test could not see this.
#[test]
fn the_pixel_size_printed_for_a_png_is_the_pixel_size_in_the_file() {
    let dir = scratch("export-png-dims");
    write(&dir, "map.gb", &genbank("map", "AAAACCCCGGGGTTTT", true));

    let out = run(
        &dir,
        &[
            "export", "map.gb", "--png", "--width", "720", "--height", "540", "--mm", "96",
            "--dpi", "300",
        ],
    );
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");

    let line = err
        .lines()
        .find(|l| l.contains(" px at "))
        .unwrap_or_else(|| panic!("no pixel-size line in:\n{err}"));
    let printed: Vec<u32> = line
        .split_whitespace()
        .filter_map(|w| w.parse::<u32>().ok())
        .take(2)
        .collect();
    assert_eq!(printed.len(), 2, "{line:?} does not state two dimensions");

    // IHDR: 8 bytes of signature, then a chunk header of length + type, then
    // width and height. It is the first chunk by the spec, so the offsets are
    // fixed.
    let png = std::fs::read(dir.join("map.png")).expect("the export wrote map.png");
    let at = |i: usize| u32::from_be_bytes(png[i..i + 4].try_into().unwrap());
    assert_eq!(&png[12..16], b"IHDR", "the first chunk is not IHDR");
    let (w, h) = (at(16), at(20));

    assert_eq!(
        (printed[0], printed[1]),
        (w, h),
        "pl printed {} x {} px and IHDR says {w} x {h}",
        printed[0],
        printed[1]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- `pl update`: the one verb that uses a network ---------------------------
//
// None of these make a request, and that is a constraint on the tests rather
// than a gap in them. A test that fetched the real release page would be
// testing GitHub's availability, would fail on the CI leg with no egress, and
// would be the only thing in this suite that phones home. What can be tested
// without a network turns out to be most of what matters: the argument
// handling, the disclosure, and — the one that would otherwise go unnoticed —
// which code in this binary is allowed to reach a socket at all. The flow
// itself is tested end to end in `crates/pl-update`, against an in-memory
// server that serves signed bytes.

/// Every line in `bins/pl` that can open a socket is inside `cmd_update`.
///
/// PROVEN TO FAIL by hoisting the `pl_update::Curl::default()` line out of
/// `cmd_update` into a helper above it, and separately by adding a
/// `pl_update::curl_available()` call to `cmd_info`. Both went red here and
/// **green everywhere else in this file** — `pl info` still summarises a file
/// correctly with a network probe bolted onto it, which is exactly why this
/// reads the source instead of running the binary. The shape is
/// `crates/pl-update/tests/handoff.rs`'s and `crates/pl-design/tests/purity.rs`':
/// a claim about what code does NOT do cannot be established by calling it.
///
/// The rule it enforces is also a style rule, deliberately: the crate must be
/// spelled `pl_update::` at each use site. A `use pl_update::check;` at the top
/// of the file would put the name outside `cmd_update` and fail here, which is
/// the intended answer — an import is how the network surface stops being
/// greppable.
#[test]
fn only_the_update_verb_can_reach_the_network() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("main.rs"),
    )
    .expect("bins/pl/src/main.rs");
    let lines: Vec<&str> = src.lines().collect();

    // rustfmt puts every item in this file at column 0 and closes it with a `}`
    // at column 0. That is a sounder boundary than counting braces, which any
    // `{}` in this function's format strings would throw off.
    let start = lines
        .iter()
        .position(|l| l.starts_with("fn cmd_update("))
        .expect("`fn cmd_update` is gone, so this test now proves nothing");
    let end = start
        + 1
        + lines[start + 1..]
            .iter()
            .position(|l| *l == "}")
            .expect("cmd_update has no closing `}` at column 0");

    let mut outside = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        // The prose has to be able to name the crate in order to say where it
        // is allowed. Doc comments and ordinary comments both start `//`.
        if line.trim_start().starts_with("//") {
            continue;
        }
        if line.contains("pl_update") && !(start..=end).contains(&i) {
            outside.push(format!("{}: {}", i + 1, line.trim()));
        }
    }
    assert!(
        outside.is_empty(),
        "`pl` may reach the network only from `cmd_update` (lines {}..={}); these are \
         outside it:\n  {}\nIf one is a `use`, spell the call `pl_update::` at its site \
         instead -- an import is how this stops being greppable.",
        start + 1,
        end + 1,
        outside.join("\n  ")
    );

    // Not vacuous: the calls really are in there. A `cmd_update` that had
    // stopped calling the crate entirely would satisfy every assertion above,
    // and so would deleting the verb.
    let body = lines[start..=end].join("\n");
    for call in [
        "pl_update::Curl",
        "pl_update::check(",
        "pl_update::fetch_and_verify(",
    ] {
        assert!(
            body.contains(call),
            "cmd_update no longer calls {call}, so the range this test guards is empty"
        );
    }
}

/// A verb that takes no options refuses one instead of ignoring it, which is
/// what `pl --help` says of every verb its GLOBAL `--json` entry omits.
///
/// PROVEN TO FAIL on 2026-09-03: `pl licences --json` exited 0 and printed the
/// whole licence text, and `pl cut-adapter --json` exited 0 in silence, while
/// `pl methods --json` refused with "unknown option '--json'; this command
/// takes no options". Both functions bound `_args` and never called
/// `parse_args`.
#[test]
fn a_verb_that_takes_no_options_refuses_one_rather_than_ignoring_it() {
    let dir = scratch("no-options-refused");
    // The two verbs that did not, until 2026-09-03: both bound `_args` and
    // never called `parse_args`, so `pl licences --json` printed the whole
    // licence text and exited 0 while `pl methods --json` refused. `pl
    // --help`'s GLOBAL entry says every verb it does not list refuses
    // `--json`; these are the two that made that sentence false. The unit
    // test `the_global_json_entry_names_every_verb_that_takes_it` now asserts
    // every `cmd_*` calls `parse_args`; this one asserts what a user sees.
    for verb in ["licences", "cut-adapter"] {
        let out = run(&dir, &[verb, "--json"]);
        assert!(
            !out.status.success(),
            "`pl {verb} --json` exited 0; a verb that takes no options must \
             refuse one, not ignore it"
        );
        let err = stderr(&out);
        assert!(
            err.contains("unknown option '--json'"),
            "`pl {verb} --json` failed without naming the option: {err:?}"
        );
    }
    // ...and the verb still works when asked properly. `cut-adapter` reads
    // stdin, which `run` gives as empty, so only `licences` is exercised here.
    let out = run(&dir, &["licences"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

/// `pl licences` carries the notice OFL clause 2 attaches to the faces this
/// binary embeds, to a user who has nothing beside the executable.
///
/// NOTICE's "HOW THE OBLIGATION TRAVELS" paragraph carried the absence of this
/// block as the last clause still owed, from 2026-08-04 to 2026-09-03. Run
/// through the binary, not the function, because the obligation is on the
/// copy a user runs.
#[test]
fn licences_prints_the_fonts_this_binary_embeds() {
    let dir = scratch("licences-fonts");
    let out = run(&dir, &["licences"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    for want in [
        "FONTS EMBEDDED IN THIS PROGRAM",
        "Liberation Sans Bold",
        "Copyright (c) 2012 Red Hat, Inc.",
        "Reserved Font Name",
        "SIL OPEN FONT LICENSE Version 1.1",
    ] {
        assert!(
            text.contains(want),
            "`pl licences` does not print {want:?}:\n{text}"
        );
    }
    // ...and still the data block it has always printed, in front of it.
    let data = text
        .find("ATTRIBUTION")
        .expect("the annotation-data block is gone");
    let fonts = text.find("FONTS EMBEDDED").expect("the font block is gone");
    assert!(
        data < fonts,
        "the font block moved in front of the data block"
    );
    assert!(text.contains("features/NOTICE"), "{text}");
}

/// A mistyped flag is refused **before** anything is fetched.
///
/// This is not a copy of `a_mistyped_option_stops_the_run_instead_of_changing_the_answer`.
/// There, the cost of parsing late would be a wrong answer. Here it would be a
/// request: `pl update --chek` that parsed its argv after calling `check` would
/// still exit non-zero with "unknown option", having already told github.com
/// that this machine exists and runs this version. The refusal has to happen
/// first, and the only externally visible evidence of the ordering is the
/// absence of a transport error in a run that cannot succeed.
#[test]
fn a_mistyped_update_flag_is_refused_before_anything_is_fetched() {
    let dir = scratch("update-typo");
    for args in [
        vec!["update", "--chek"],
        vec!["update", "--check", "--force"],
        vec!["update", "--to"], // valued, with the value missing
    ] {
        let out = run(&dir, &args);
        assert!(!out.status.success(), "{args:?} must not succeed");
        let err = stderr(&out);
        assert!(
            err.contains("unknown option") || err.contains("needs a value"),
            "{args:?}: {err}"
        );
        assert!(
            !err.contains("could not fetch") && !err.contains("curl"),
            "{args:?} reached the network before it read its own arguments: {err}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `pl update ~/Downloads` is a mistake, and a silently ignored one would write
/// somewhere else and report success.
#[test]
fn update_refuses_a_stray_positional_rather_than_downloading_elsewhere() {
    let dir = scratch("update-positional");
    let out = run(&dir, &["update", "somewhere"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("no file arguments"), "{err}");
    // It names the flag that does what the user meant.
    assert!(err.contains("--to"), "{err}");
    assert!(
        !err.contains("could not fetch") && !err.contains("curl"),
        "refused only after asking the network: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a check discloses is readable **without** running one.
///
/// `pl update --help` is the only place a user can find out what a check sends
/// before choosing to send it, so these sentences going missing is a defect in
/// the product and not a documentation nit. Every phrase below is a specific
/// claim about the request, and this fails if any is deleted or softened away.
#[test]
fn the_update_verb_states_what_it_sends_before_it_is_run() {
    let dir = scratch("update-help");
    let out = run(&dir, &["update", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    for required in [
        "UPDATE OPTIONS",
        "WHAT THIS SENDS",
        "no sequence",      // what is not in the request
        "IP address",       // what unavoidably is
        "installs nothing", // requirement 4, in the user's words
    ] {
        assert!(
            text.contains(required),
            "`pl update --help` no longer says {required:?}"
        );
    }
    // And the verb is listed where somebody would look for it, with the fact
    // that it is the networked one attached to the name rather than buried.
    assert!(
        text.contains("pl update"),
        "the verb is not in the usage list"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// PROVEN TO FAIL at 6fed367, on every one of the four formats.
///
/// `pl export` named the circle in the call — `circular_svg`, `circular_pdf`,
/// `circular_png_at` — so a PCR product, a linearised vector, a gene fragment,
/// a gBlock, every FASTA and every assembly came out as a C-shaped ring with a
/// gap in it. The gap is honest about topology and is still the wrong picture
/// for a construct laid out end to end.
///
/// EPS is in here because it is the one format that would have diverged
/// silently: it has always gone through `pl_draw::scene` and so followed the
/// topology the moment `scene` learned to, while the other three would have
/// kept drawing rings — `--eps` and `--pdf` of the same file, different shapes.
#[test]
fn a_linear_molecule_exports_a_linear_figure_in_every_format() {
    let dir = scratch("export-linear");
    // A FASTA, because that is the population: every FASTA opens linear.
    write(
        &dir,
        "product.fa",
        &format!(">amplicon\n{}\n", "ACGTAGCTAGCTAGGCTA".repeat(60)),
    );
    write(&dir, "vector.gb", &one_site_plasmid());

    let svg = run(&dir, &["export", "product.fa", "--stdout"]);
    assert!(svg.status.success(), "{}", stderr(&svg));
    let svg = stdout(&svg);
    assert!(
        !svg.contains("<circle"),
        "a PCR product was exported as a circle"
    );
    // No arc command in any path. Scanned inside the `d` attributes rather than
    // over the whole document, because the root's `font-family` names Arial and
    // a bare search for "A" finds it.
    for d in svg.split("d=\"").skip(1) {
        let d = d.split('"').next().unwrap_or("");
        assert!(
            !d.contains('A') && !d.contains('a'),
            "a PCR product was exported with an arc in its path data: {d:.80}"
        );
    }

    // The circular control, in the same run: it must be untouched.
    let round = run(&dir, &["export", "vector.gb", "--stdout"]);
    assert!(round.status.success(), "{}", stderr(&round));
    assert!(
        stdout(&round).contains("<circle"),
        "a plasmid stopped being drawn as a ring"
    );

    // EPS and PDF are binary-ish, so they are judged on shape rather than text:
    // the ring's arcs reach `curveto` in PostScript and `c` in PDF, both of
    // which a track has no use for. Written to a file, since `--stdout` is one
    // stream and these are two runs.
    let eps = run(&dir, &["export", "product.fa", "--eps", "--stdout"]);
    assert!(eps.status.success(), "{}", stderr(&eps));
    let eps = stdout(&eps);
    assert!(
        !eps.contains("curveto"),
        "the EPS of a linear molecule still has arcs in it"
    );
    assert!(eps.contains("%!PS-Adobe"), "not an EPS at all");

    for fmt in ["--pdf", "--png"] {
        let o = run(&dir, &["export", "product.fa", fmt, "--outdir", "."]);
        assert!(o.status.success(), "{fmt}: {}", stderr(&o));
    }
    let pdf = std::fs::read(dir.join("product.pdf")).expect("the pdf");
    assert!(pdf.starts_with(b"%PDF-"), "not a PDF");
    assert!(std::fs::read(dir.join("product.png"))
        .expect("the png")
        .starts_with(&[0x89, b'P']));
}

/// The same input, rendered by two separate PROCESSES, is the same bytes.
///
/// `crates/pl-draw` asserts this inside one process, over eight renders in a
/// loop. That is the weaker half: everything a process shares with itself — a
/// warmed allocator, one `RandomState` seed, one CPU's floating-point mode, one
/// environment and locale — is held constant by construction, so a figure that
/// depended on any of it would pass. Two `pl export` runs share none of it. A
/// `HashSet` anywhere in the label path reseeds per process and would show here
/// and nowhere else.
///
/// BOTH SHAPES, all four formats. The track is the newer path and the one with
/// a packer that re-offers what a row dropped, but the ring is here too: a
/// determinism check covering only the new code says nothing about a regression
/// in the old one.
///
/// `--sites all` on a deliberately cramped canvas, so the runs go through the
/// DROP paths. `labels_hidden`, `sites_hidden` and the spill `place_rows`
/// returns are lists a reader sees and an unordered container would reorder
/// without moving one coordinate; a figure with room for everything never
/// reaches them.
#[test]
fn two_processes_render_the_same_molecule_to_the_same_bytes() {
    let dir = scratch("export-determinism");
    // A circle and a line out of the same bases, so the only difference between
    // the two figures is the one under test.
    let mut seq = "ACGT".repeat(300);
    for (i, site) in ["GAATTC", "GGATCC", "AAGCTT", "GCGGCCGC", "CTCGAG", "GTCGAC"]
        .iter()
        .enumerate()
    {
        let at = 200 + i * 180;
        seq.replace_range(at..at + site.len(), site);
    }
    write(&dir, "round.gb", &genbank("pDETERM", &seq, true));
    write(&dir, "flat.gb", &genbank("pDETERM", &seq, false));
    for out in ["a", "b"] {
        std::fs::create_dir_all(dir.join(out)).expect("an output directory");
    }

    for stem in ["round", "flat"] {
        let input = format!("{stem}.gb");
        for (flag, ext) in [
            ("", "svg"),
            ("--pdf", "pdf"),
            ("--eps", "eps"),
            ("--png", "png"),
        ] {
            let mut wrote: Vec<Vec<u8>> = Vec::new();
            let mut said: Vec<String> = Vec::new();
            for out in ["a", "b"] {
                let mut args: Vec<&str> = vec![
                    "export", &input, "--sites", "all",
                    // Cramped on purpose: this is what reaches the drop paths.
                    "--width", "300", "--height", "220", "--outdir", out,
                ];
                if !flag.is_empty() {
                    args.push(flag);
                }
                let o = run(&dir, &args);
                assert!(o.status.success(), "{stem} {ext}: {}", stderr(&o));
                // stderr carries the disclosure line, whose counts are read off
                // the figure, so it is part of what has to be stable.
                said.push(stderr(&o));
                wrote.push(
                    std::fs::read(dir.join(out).join(format!("{stem}.{ext}")))
                        .unwrap_or_else(|e| panic!("{out}/{stem}.{ext}: {e}")),
                );
            }
            assert!(!wrote[0].is_empty(), "{stem} {ext} wrote nothing");
            assert_eq!(said[0], said[1], "{stem} {ext}: what it said moved");
            assert_eq!(
                wrote[0],
                wrote[1],
                "{stem} {ext}: two processes wrote {} and {} bytes and they differ",
                wrote[0].len(),
                wrote[1].len()
            );
        }
    }
    // And the two shapes really were two different pictures, so the loop above
    // did not compare one figure with itself twice.
    let round = std::fs::read(dir.join("a").join("round.svg")).expect("round");
    let flat = std::fs::read(dir.join("a").join("flat.svg")).expect("flat");
    assert_ne!(round, flat, "the ring and the track came out identical");
    let _ = std::fs::remove_dir_all(&dir);
}
