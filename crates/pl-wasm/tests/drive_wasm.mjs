/**
 * Drive the real .wasm module and check it agrees with the native binary.
 *
 * The Rust unit tests exercise the functions; they do not exercise the ABI.
 * This loads the actual WebAssembly, pushes real files through it, and compares
 * the result against `pl.exe --json` — the same source compiled for a different
 * target. Disagreement means the wasm boundary is lying somewhere.
 *
 * Usage:
 *   node drive_wasm.mjs <path-to-pl_wasm.wasm> <path-to-pl(.exe)> <corpus-dir>
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, extname, basename } from "node:path";
import { execFileSync } from "node:child_process";

const [wasmPath, plPath, corpus] = process.argv.slice(2);
if (!wasmPath || !plPath) {
  console.error("usage: node drive_wasm.mjs <pl_wasm.wasm> <pl binary> [corpus dir]");
  process.exit(2);
}

/* ---------- load ---------- */

const bytes = readFileSync(wasmPath);
const module = await WebAssembly.compile(bytes);

const imports = WebAssembly.Module.imports(module);
if (imports.length) {
  console.log(`  note: module declares ${imports.length} import(s):`);
  for (const i of imports) console.log(`    ${i.module}.${i.name} (${i.kind})`);
} else {
  console.log("  module has zero imports — nothing to stub, nothing to trust");
}

const instance = await WebAssembly.instantiate(module, {});
const w = instance.exports;

/* ---------- ABI helpers ---------- */

// Any call may grow memory, which detaches previously-created views, so every
// helper re-creates its own. This is the one sharp edge of the hand-rolled ABI.
const u8 = () => new Uint8Array(w.memory.buffer);

function out() {
  const ptr = w.pl_out_ptr();
  const len = w.pl_out_len();
  return u8().slice(ptr, ptr + len);
}
const outText = () => new TextDecoder().decode(out());
const outJson = () => JSON.parse(outText());

/* `pl_alloc` returns null on failure rather than trapping, so every caller has
   to check: address 0 is inside linear memory and `set(..., 0)` overwrites the
   module's own data without throwing. This harness is the in-tree implementer of
   the ABI contract, so it demonstrates the check the contract requires rather
   than relying on allocations here being small enough never to fail. */
function alloc(len) {
  const ptr = w.pl_alloc(len);
  if (ptr === 0) throw new Error(`pl_alloc(${len}) returned null`);
  return ptr;
}

function open(buf) {
  const ptr = alloc(buf.length);
  u8().set(buf, ptr);
  const rc = w.pl_open(ptr, buf.length);
  w.pl_free(ptr, buf.length);
  return rc;
}

function withStr(s, fn) {
  const b = new TextEncoder().encode(s);
  const ptr = alloc(b.length);
  u8().set(b, ptr);
  try {
    return fn(ptr, b.length);
  } finally {
    w.pl_free(ptr, b.length);
  }
}

/* ---------- test harness ---------- */

let failures = 0;
const T = (label, cond, extra = "") => {
  console.log(`  ${cond ? "PASS" : "FAIL"}  ${label}${extra ? "  — " + extra : ""}`);
  if (!cond) failures++;
};

console.log(`\n=== module ===`);
T("ABI version is 1", w.pl_abi_version() === 1, String(w.pl_abi_version()));
T(
  "exports the expected surface",
  ["pl_alloc", "pl_free", "pl_open", "pl_out_ptr", "pl_out_len", "pl_sequence",
   "pl_digest_json", "pl_blocks_json", "pl_to_genbank", "pl_to_fasta",
   "pl_locus_name", "pl_rotate", "memory"].every(n => n in w)
);

console.log(`\n=== behaviour on hand-made input ===`);
{
  const fasta = new TextEncoder().encode(">demo test\nACGTacgtNN\n");
  T("opens FASTA", open(fasta) === 0);
  const j = outJson();
  T("reports format", j.format === "FASTA", j.format);
  T("preserves case", j.lowercase === 4, `lowercase=${j.lowercase}`);
  T("counts ambiguous bases", j.ambiguous === 2, `ambiguous=${j.ambiguous}`);
  w.pl_sequence();
  T("sequence round-trips byte-for-byte", new TextDecoder().decode(out()) === "ACGTacgtNN");
}
{
  // A multi-record file, over the real ABI. `pl_open` used to call
  // `pl_fileio::load`, which drops the LoadReport in its own body, so a
  // 3-record FASTA opened as record 1 with no record count anywhere in the
  // JSON — and `pl_to_genbank` then wrote that one record out as the file at
  // rc 0, an operation `pl convert` refuses outright. Checked here and not
  // only in the Rust unit tests because this is the surface the page uses.
  const multi = new TextEncoder().encode(
    ">plasmidA first\nGAATTCAAAAAAAAAAAAAAAA\n" +
    ">plasmidB second\nGGATCCTTTTTTTTTTTTTTTT\n" +
    ">plasmidC third\nAAAAAAAAAAAAAAAAAAAAAA\n");
  T("opens a multi-record FASTA", open(multi) === 0);
  const j = outJson();
  T("shows the first record", j.name === "plasmidA", j.name);
  T("says how many records the file held", j.recordsInFile === 3, `recordsInFile=${j.recordsInFile}`);
  T("says FASTA never declared a topology", j.topologyDeclared === false, String(j.topologyDeclared));

  const rcGb = withStr("multi.fa", (p, n) => w.pl_to_genbank(p, n, 26, 6, 2026));
  T("GenBank export refuses rather than writing record 1", rcGb === 1, `rc=${rcGb}`);
  // The page throws these return codes away and downloads the buffer, so the
  // refusal has to be *in the buffer* or it is a silent no-op there.
  T("and the refusal is in the output buffer", outText().includes("3 records"), outText().slice(0, 90));
  const rcFa = withStr("multi.fa", (p, n) => w.pl_to_fasta(p, n, 70));
  T("FASTA export refuses too", rcFa === 1 && !outText().includes("GAATTC"), outText().slice(0, 90));
}
{
  // ...and the refusal fires on truncation and nothing else.
  open(new TextEncoder().encode(">solo only one\nACGTACGTACGT\n"));
  T("a single-record file reports one record", outJson().recordsInFile === 1);
  const rc = withStr("solo.fa", (p, n) => w.pl_to_fasta(p, n, 70));
  T("and still exports", rc === 0 && outText().includes("ACGTACGTACGT"), outText().slice(0, 60));
}
{
  T("rejects rubbish with rc=1", open(new TextEncoder().encode("nonsense")) === 1);
  T("error is JSON", !!outJson().error, outText().slice(0, 60));
}
{
  const abif = new Uint8Array([0x41, 0x42, 0x49, 0x46, 0, 1, 2, 3]);
  open(abif);
  T("names ABIF in the error", outJson().error.includes("ABIF"), outJson().error);
}
{
  // A colour containing '#' and a multibyte label: the two things that used to
  // panic. A panic in wasm traps, so this would throw rather than fail softly.
  const gb =
    "LOCUS       x                        12 bp    DNA     circular SYN 01-JAN-2026\n" +
    "FEATURES             Location/Qualifiers\n" +
    "     CDS             complement(1..6)\n" +
    '                     /label="δ subunit"\n' +
    '                     /ApEinfo_fwdcolor="#1a2b3c"\n' +
    "ORIGIN\n        1 acgtacgtacgt\n//\n";
  let threw = null;
  try {
    open(new TextEncoder().encode(gb));
  } catch (e) {
    threw = e;
  }
  T("multibyte label + hex colour does not trap", threw === null, threw?.message ?? "");
  const j = outJson();
  T("multibyte label survives the boundary", j.features?.[0]?.name === "δ subunit", j.features?.[0]?.name);
  T("colour parsed", j.features?.[0]?.color === "#1a2b3c", j.features?.[0]?.color);
  T("strand parsed", j.features?.[0]?.strand === "-", j.features?.[0]?.strand);
}
{
  withStr("my plasmid v2.dna", (p, n) => w.pl_locus_name(p, n));
  T("locus name sanitised", outText() === "my_plasmid_v2", outText());
}

/* ---------- corpus: wasm vs native ---------- */

if (!corpus) {
  console.log("\n(no corpus given; skipping the comparison against the native binary)");
} else {
  console.log(`\n=== wasm vs native binary over the corpus ===`);
  const files = [];
  (function walk(d, depth) {
    if (depth > 10) return;
    let entries;
    try { entries = readdirSync(d, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p, depth + 1);
      else if (extname(e.name).toLowerCase() === ".dna") files.push(p);
    }
  })(corpus, 0);
  files.sort();

  let agree = 0, disagree = 0, totalFeat = 0, totalBp = 0;
  for (const f of files) {
    const buf = readFileSync(f);
    if (open(buf) !== 0) {
      console.log(`    wasm refused ${basename(f)}: ${outText().slice(0, 90)}`);
      disagree++;
      continue;
    }
    const wj = outJson();

    // The native binary, same source, different target.
    const nj = JSON.parse(execFileSync(plPath, ["info", "--json", f], {
      encoding: "utf8", maxBuffer: 1 << 28,
    }))[0];

    const problems = [];
    if (wj.bp !== nj.bp) problems.push(`bp ${wj.bp} vs ${nj.bp}`);
    if (wj.circular !== nj.circular) problems.push("topology");
    if (wj.lowercase !== nj.lowercase) problems.push(`lowercase ${wj.lowercase} vs ${nj.lowercase}`);
    if (wj.features.length !== nj.n_features) {
      problems.push(`features ${wj.features.length} vs ${nj.n_features}`);
    }
    if (wj.primers.length !== nj.n_primers) {
      problems.push(`primers ${wj.primers.length} vs ${nj.n_primers}`);
    }
    const wSites = wj.primers.reduce((n, p) => n + p.sites.length, 0);
    if (wSites !== nj.n_binding_sites) {
      problems.push(`binding sites ${wSites} vs ${nj.n_binding_sites}`);
    }
    for (let i = 0; i < Math.min(wj.features.length, nj.features.length); i++) {
      const a = wj.features[i], b = nj.features[i];
      if (a.name !== b.name || a.kind !== b.kind || a.start !== b.start ||
          a.end !== b.end || a.strand !== b.strand || a.segments.length !== b.segments) {
        problems.push(`feature ${i} '${b.name}' differs`);
        break;
      }
    }

    // The bases themselves, not just the count.
    w.pl_sequence();
    const seq = out();
    if (seq.length !== wj.bp) problems.push(`pl_sequence gave ${seq.length}, summary said ${wj.bp}`);

    totalFeat += wj.features.length;
    totalBp += wj.bp;
    if (problems.length) {
      disagree++;
      console.log(`    DIFFER ${basename(f).slice(0, 44)}: ${problems.join("; ")}`);
    } else {
      agree++;
    }
  }

  console.log(`\n  files compared : ${files.length}`);
  console.log(`  agree          : ${agree}`);
  console.log(`  disagree       : ${disagree}`);
  console.log(`  features       : ${totalFeat}`);
  console.log(`  bases          : ${totalBp.toLocaleString()}`);
  if (disagree) failures++;

  // GenBank produced in wasm must be byte-identical to the native output.
  console.log(`\n=== GenBank output: wasm vs native, byte for byte ===`);
  let same = 0, differ = 0;
  for (const f of files) {
    const buf = readFileSync(f);
    if (open(buf) !== 0) continue;
    const title = basename(f);
    withStr(title, (p, n) => w.pl_to_genbank(p, n, 26, 6, 2026));
    const fromWasm = outText();
    // Native uses today's date, so compare with the date line normalised.
    const fromNative = execFileSync(plPath, ["convert", f, "--to", "genbank", "--stdout"], {
      encoding: "utf8", maxBuffer: 1 << 28,
    });
    const strip = s => s.replace(/^(LOCUS.*?)\d{2}-[A-Z]{3}-\d{4}\s*$/m, "$1DATE");
    if (strip(fromWasm) === strip(fromNative)) same++;
    else {
      differ++;
      if (differ <= 3) {
        const a = strip(fromWasm).split("\n"), b = strip(fromNative).split("\n");
        const i = a.findIndex((l, k) => l !== b[k]);
        console.log(`    ${basename(f).slice(0, 40)} differs at line ${i + 1}:`);
        console.log(`      wasm  : ${JSON.stringify(a[i]?.slice(0, 80))}`);
        console.log(`      native: ${JSON.stringify(b[i]?.slice(0, 80))}`);
      }
    }
  }
  console.log(`  identical: ${same}/${files.length}`);
  if (differ) failures++;
}

console.log(`\n${failures === 0 ? "ALL WASM CHECKS PASSED" : failures + " WASM CHECK(S) FAILED"}`);
process.exit(failures === 0 ? 0 : 1);
