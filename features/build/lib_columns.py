"""The schema, in one place.

Kept in step with `crates/pl-features/src/lib.rs` by a test that reads this
file, so the builder and the reader cannot drift apart silently — which is how
a curated database ends up with a column nobody parses.
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
