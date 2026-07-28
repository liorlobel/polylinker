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
