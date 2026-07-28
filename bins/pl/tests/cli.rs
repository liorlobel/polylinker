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
