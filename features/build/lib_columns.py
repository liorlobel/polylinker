"""The schema, in one place.

Kept in step with `crates/pl-features/src/lib.rs` by
`crates/pl-features/tests/schema_pin.rs`, which parses THIS FILE and compares
both lists against the Rust constants — so a rename on either side fails, and
the failure names which side moved.

That test is new, and this docstring asserted it for a while before it existed.
The pin was real but indirect and one-directional: `build.py` writes the header
out of `FEATURE_COLUMNS`, and the Rust loader compares the header of the
generated `features.tsv` against its own constant. A Rust-side rename therefore
failed at once, and a Python-side rename left the entire suite green until
somebody happened to rebuild — which is exactly the silent drift the sentence
claimed to prevent.
"""

FEATURE_COLUMNS = [
    "id",
    "name",
    "aliases",
    "class",
    "genbank_key",
    "reference_nt",
    "reference_aa",
    "boundary_rule",
    "boundary_evidence",
    "description",
    "review_status",
    "curator",
    "date_added",
    "patent_flag",
    "notes",
]

PROVENANCE_COLUMNS = [
    "record_id",
    "field",
    "source_db",
    "source_accession",
    "licence",
    "url",
    "retrieved",
    "sha256",
]

SIGNOFF_COLUMNS = [
    "record_id",
    "review_status",
    "curator",
    "signed_date",
    "content_sha256",
    "note",
]

# The columns a curator's signature covers, in digest order.
#
# Written out rather than computed as FEATURE_COLUMNS minus PROVENANCE_EXEMPT,
# even though it equals that today. Coupling the digest to a mutable set means a
# future edit to that set either silently invalidates every signature in the
# repository or silently stops covering a column, and neither announces itself.
#
# The four absentees, each for its own reason:
#   id             the key the signature is ON, not content it covers.
#   review_status  what the signature SETS. Including it would make the digest
#   curator        depend on its own outcome.
#   date_added     the build clock. write_outputs() stamps it on every row on
#                  every run, so a whole-row digest would invalidate every
#                  sign-off in the repository on every build. This exclusion is
#                  what makes the scheme possible at all, and check_signoff.py
#                  carries an inverted control that proves it.
#
# description and notes are deliberately IN. What a curator signs is the claim
# made to a user, and 'reviewed' is defined as having written the description
# from the primary source; a signature that survived arbitrary rewriting of the
# prose would look like an approval of text nobody has read.
SIGNED_COLUMNS = [
    "name",
    "aliases",
    "class",
    "genbank_key",
    "reference_nt",
    "reference_aa",
    "boundary_rule",
    "boundary_evidence",
    "description",
    "patent_flag",
    "notes",
]
