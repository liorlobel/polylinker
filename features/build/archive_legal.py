#!/usr/bin/env python3
"""Archive the licence evidence into `legal/`, so the sourcing record self-evidences.

`features/SOURCING.md` section 2 Stage 0.3 requires this and gives the reason:
`www.uniprot.org/help/license` and `ebi.ac.uk/ena/browser/about/policies` are
JavaScript shells with **zero licence text**, so a URL in a document is not
evidence of anything — a reader who follows it sees an empty page. The
machine-readable equivalents are archived here as files, with a sha256 and a
retrieval date, and Risk 9 ("the evidence package does not self-evidence") is
what happens when they are not.

`legal/` existed and was empty, while `features/NOTICE` told readers the
evidence was in SOURCING.md and SOURCING.md's own preamble said "I have not
independently re-fetched any URL in this document". Every URL below has now been
fetched, and the hash of what came back is recorded beside it.

This is EVIDENCE, not data. Nothing it downloads enters `features.tsv`, which is
why it has its own host list rather than `build.ALLOWED_FETCH_HOSTS`: those are
the hosts cleared to supply *sequences*, and these are the hosts that publish the
terms under which those sequences are supplied. Keeping the two lists apart means
adding a licence page cannot quietly add a data source.

Usage
-----
    python features/build/archive_legal.py
    python features/build/archive_legal.py --check   # verify, fetch nothing
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
LEGAL = REPO / "legal"

# See build.py: this URL is how a server operator identifies the fetcher, and
# `polylinker/polylinker` is an organisation that does not exist.
UA = "polylinker-legal-archive/0.1 (https://github.com/liorlobel/polylinker)"
TODAY = time.strftime("%Y-%m-%d")

# (filename, url, why this exact URL and not the human-facing one)
ITEMS = [
    (
        "uniprot-help-license.json",
        "https://rest.uniprot.org/help/license",
        "The human page at www.uniprot.org/help/license is a JavaScript shell "
        "with no licence text in the HTML. This REST endpoint returns the same "
        "help article as JSON, which is what can actually be archived and hashed.",
    ),
    (
        "uniprot-ftp-LICENSE.txt",
        "https://ftp.uniprot.org/pub/databases/uniprot/LICENSE",
        "The operative licence file that ships with the distribution. "
        "SOURCING.md section 1 disagreement 4 turns on a per-copy notice "
        "condition stated here and nowhere on the website.",
    ),
    (
        "uniprot-knowledgebase-README.txt",
        "https://ftp.uniprot.org/pub/databases/uniprot/current_release/knowledgebase/complete/README",
        "The knowledgebase README, which is where the distribution states what "
        "the release contains and under what terms.",
    ),
    (
        "rfam-COPYING.txt",
        "https://ftp.ebi.ac.uk/pub/databases/Rfam/CURRENT/COPYING",
        "Rfam's CC0 declaration as shipped, rather than as summarised by a "
        "badge. SOURCING.md section 1 records that the challenge round narrowed "
        "Rfam from GO to GO_WITH_CAVEAT on the strength of what this does and "
        "does not cover.",
    ),
    (
        "ncbi-policies.html",
        "https://www.ncbi.nlm.nih.gov/home/about/policies/",
        "NCBI's Disclaimer and Copyright notice. The old "
        "/About/disclaimer.html now redirects here, and features/NOTICE points "
        "users at this URL, so the page it points at is archived.",
    ),
    (
        "nlm-terms-and-conditions.html",
        "https://www.nlm.nih.gov/web_policies.html",
        "The NLM Terms and Conditions governing the FTP servers this build "
        "pulls AMRFinderPlus from. SOURCING.md's first named disagreement is "
        "that the prober never opened this page, and that reading it flips "
        "attribution_required from false to TRUE.",
    ),
    (
        "wwpdb-usage-policies.html",
        "https://www.wwpdb.org/about/usage-policies",
        "The wwPDB Usage Policy, which is the operative statement for PDB "
        "archive data and the evidence behind clearing `wwpdb / CC0-1.0` in "
        "SOURCING.md section 1. It is archived rather than cited because it is "
        "the ONLY page in this set that states the licence for the source the "
        "curated peptide references depend on, and unlike RCSB's own policies "
        "page it puts the licence text in the HTML rather than behind script. "
        "The distinction it draws matters and is why the fetch is narrow: the "
        "CC0 dedication covers the deposited archive data, while RCSB's own "
        "website content is separately CC BY 4.0, so this build reads only the "
        "deposited one-letter sequence out of a polymer entity.",
    ),
]

ALLOWED_HOSTS = {
    "rest.uniprot.org",
    "ftp.uniprot.org",
    "ftp.ebi.ac.uk",
    "www.ncbi.nlm.nih.gov",
    "www.nlm.nih.gov",
    "www.wwpdb.org",
}


def fetch(url: str) -> bytes:
    host = urllib.parse.urlsplit(url).hostname or ""
    if host not in ALLOWED_HOSTS:
        raise SystemExit(f"{host!r} is not a licence-evidence host this script may fetch")
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true",
                    help="verify the archive against its manifest without fetching")
    args = ap.parse_args()

    manifest_path = LEGAL / "MANIFEST.json"

    if args.check:
        if not manifest_path.exists():
            print("legal/MANIFEST.json is absent: nothing has been archived")
            return 1
        manifest = json.loads(manifest_path.read_text(encoding="utf8"))
        bad = 0
        for item in manifest["files"]:
            p = LEGAL / item["file"]
            if not p.exists():
                print(f"  MISSING {item['file']}")
                bad += 1
                continue
            got = hashlib.sha256(p.read_bytes()).hexdigest()
            ok = got == item["sha256"]
            print(f"  {'ok  ' if ok else 'HASH'} {item['file']}  {item['retrieved']}")
            bad += 0 if ok else 1
        if bad:
            print(f"{bad} problem(s): the archive does not match its own manifest")
            return 1
        print(f"{len(manifest['files'])} archived file(s), all matching their recorded sha256")
        return 0

    LEGAL.mkdir(parents=True, exist_ok=True)
    files, failed = [], []
    for name, url, why in ITEMS:
        try:
            data = fetch(url)
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            print(f"  FAIL {name}: {e}")
            failed.append(name)
            continue
        (LEGAL / name).write_bytes(data)
        digest = hashlib.sha256(data).hexdigest()
        files.append({
            "file": name, "url": url, "why": why,
            "bytes": len(data), "sha256": digest, "retrieved": TODAY,
        })
        print(f"  ok   {name:36s} {len(data):>9,} bytes  {digest[:16]}...")

    manifest_path.write_text(
        json.dumps(
            {
                "what": "Licence evidence for features/SOURCING.md, archived as files "
                        "because two of the URLs it cites render nothing without "
                        "JavaScript. Retrieval date and sha256 are the point.",
                "generated_by": "features/build/archive_legal.py",
                "files": files,
            },
            indent=2,
        ) + "\n",
        encoding="utf8",
    )
    print(f"\n{len(files)} file(s) archived to {LEGAL}, manifest at {manifest_path}")
    if failed:
        print(f"{len(failed)} could not be fetched: {failed}. The archive is incomplete.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
