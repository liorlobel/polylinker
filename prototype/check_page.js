/**
 * Headless validation of the built dna-reader.html.
 *
 * Runs the real page in a real DOM, waits for the inlined WebAssembly core to
 * initialise, then drives it over real files from disk. Checks that:
 *   - the page loads, instantiates the core, and renders without throwing
 *   - the demo molecule appears and the map is drawn
 *   - format detection comes from the core, not from extensions
 *   - GenBank/FASTA/SVG export work through the wasm boundary
 *   - the page agrees with the native binary on the whole corpus
 *
 * Run tools/build-web.ps1 first; this tests the built file, not the template.
 *
 * Usage:
 *   node check_page.js [dna-glob] [genbank-glob] [path-to-pl.exe]
 */
const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");
const { JSDOM, VirtualConsole } = require("jsdom");

const HTML = path.join(__dirname, "dna-reader.html");
if (!fs.existsSync(HTML)) {
  console.error(`${HTML} not found — run tools/build-web.ps1 first`);
  process.exit(2);
}
const src = fs.readFileSync(HTML, "utf8");
if (src.includes("{{WASM_BASE64}}")) {
  console.error("that is the template, not a build — run tools/build-web.ps1");
  process.exit(2);
}

const errors = [];
const vc = new VirtualConsole()
  .on("jsdomError", e => errors.push("jsdomError: " + (e.message || e)))
  .on("error", (...a) => errors.push("console.error: " + a.join(" ")));

const dom = new JSDOM(`<!doctype html><html><head></head><body>${src}</body></html>`, {
  runScripts: "dangerously",
  pretendToBeVisual: true,
  virtualConsole: vc,
  url: "http://localhost/",
});
const { window } = dom;
const doc = window.document;

// jsdom has no atob/TextEncoder on the window in some versions, and no
// WebAssembly wiring; borrow Node's.
for (const [k, v] of Object.entries({
  atob: s => Buffer.from(s, "base64").toString("binary"),
  btoa: s => Buffer.from(s, "binary").toString("base64"),
  WebAssembly,
  TextEncoder,
  TextDecoder,
})) {
  if (!window[k]) window[k] = v;
}

let failures = 0;
function T(label, cond, extra = "") {
  console.log(`  ${cond ? "PASS" : "FAIL"}  ${label}${extra ? "  — " + extra : ""}`);
  if (!cond) failures++;
  return cond;
}

function expand(pattern, exts) {
  if (!pattern) return [];
  const star = pattern.indexOf("*");
  if (star === -1) return fs.existsSync(pattern) ? [pattern] : [];
  const root = pattern.slice(0, pattern.lastIndexOf(path.sep, star));
  const out = [];
  (function walk(d, depth) {
    if (depth > 10) return;
    let entries;
    try { entries = fs.readdirSync(d, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p, depth + 1);
      else if (exts.some(x => e.name.toLowerCase().endsWith(x))) out.push(p);
    }
  })(root, 0);
  return out.sort();
}

const F = expr => window.eval(expr);

async function main() {
  console.log("=== core initialisation ===");
  try {
    await F("READY");
  } catch (e) {
    T("wasm core initialises", false, e.message);
    console.log(`\n${failures} CHECK(S) FAILED`);
    process.exit(1);
  }
  T("wasm core initialises", true, `ABI ${F("W.pl_abi_version()")}`);
  T("core publishes its enzyme set", F("ENZYMES.length") === 50, `${F("ENZYMES.length")} enzymes`);
  T("page holds no parser of its own",
    !/function parseDna|function parseGenBank|function parseFasta/.test(src));

  // The demo load is kicked off by READY.then(...), so let that settle.
  await new Promise(r => setTimeout(r, 250));

  console.log("\n=== demo molecule on load ===");
  T("no uncaught errors", errors.length === 0, errors.slice(0, 2).join(" | "));
  const summary = doc.getElementById("summary").textContent;
  T("summary rendered", /circular/.test(summary), summary.slice(0, 60).replace(/\s+/g, " "));
  T("3180 bp reported", /3,180/.test(summary));
  T("map drawn", doc.getElementById("mapHost").querySelectorAll("path").length >= 8,
    `${doc.getElementById("mapHost").querySelectorAll("path").length} paths`);
  T("no NaN in geometry", !doc.getElementById("mapHost").innerHTML.includes("NaN"));
  T("features listed", F("CURRENT.features.length") === 10, `${F("CURRENT.features.length")} features`);
  T("unique cutters found", F("state.digest.filter(e => e.positions.length === 1).length") > 10,
    `${F("state.digest.filter(e => e.positions.length === 1).length")} unique`);
  T("demo is uppercase (no false soft-masking)", F("CURRENT.lowercase") === 0,
    `${F("CURRENT.lowercase")} lowercase bases`);

  console.log("\n=== export paths ===");
  const gb = F(`(() => { const d = new Date();
    withString(state.title, (p, n) => W.pl_to_genbank(p, n, 26, 6, 2026));
    return coreText(); })()`);
  T("GenBank export well formed", /^LOCUS {7}\S+/.test(gb) && gb.trimEnd().endsWith("//"),
    gb.split("\n")[0]);
  const fa = F(`(() => { withString(state.title, (p, n) => W.pl_to_fasta(p, n, 70)); return coreText(); })()`);
  T("FASTA export well formed", fa.startsWith(">") && fa.split("\n")[1].length === 70);
  const svg = F("mapToSvg()");
  T("SVG export produced", /^<\?xml/.test(svg) && svg.includes("<svg"), `${svg.length} chars`);
  T("SVG has no unresolved CSS variables", !svg.includes("var(--"),
    (svg.match(/var\(--[a-z-]+\)/) || ["none left"])[0]);
  T("locus name sanitised via core", F(`locusName("my plasmid v2.dna")`) === "my_plasmid_v2");
  T(".dna rewrite disabled for a GenBank source", doc.getElementById("dlBtn").disabled);

  console.log("\n=== error handling ===");
  const err = await F(`(async () => {
    try { await openBytes(new TextEncoder().encode("nonsense"), "x.dna"); return "no error"; }
    catch (e) { return e.message; }
  })()`);
  T("rubbish rejected with the core's message", /unrecognised/.test(err), err);
  const abifErr = await F(`(async () => {
    try { await openBytes(new Uint8Array([65,66,73,70,0,1,2,3]), "x.ab1"); return "no error"; }
    catch (e) { return e.message; }
  })()`);
  T("chromatogram named in the error", /ABIF/.test(abifErr), abifErr);

  console.log("\n=== real corpus, through the page ===");
  const dnaFiles = expand(process.argv[2], [".dna"]);
  const plBin = process.argv[4];
  let ok = 0, bad = 0, feats = 0, bases = 0;
  const mismatches = [];

  for (const f of dnaFiles) {
    // Hand the bytes over through a window property rather than building a
    // multi-megabyte source literal for eval.
    window.__bytes = new Uint8Array(fs.readFileSync(f));
    let d;
    try {
      d = F("coreOpen(new Uint8Array(window.__bytes))");
    } catch (e) {
      bad++;
      if (bad <= 3) console.log(`    ${path.basename(f).slice(0, 44)}: ${e.message}`);
      continue;
    }
    feats += d.features.length;
    bases += d.sequence.length;

    if (plBin && fs.existsSync(plBin)) {
      const native = JSON.parse(execFileSync(plBin, ["info", "--json", f], {
        encoding: "utf8", maxBuffer: 1 << 28,
      }))[0];
      const probs = [];
      if (d.sequence.length !== native.bp) probs.push(`bp ${d.sequence.length} vs ${native.bp}`);
      if (d.circular !== native.circular) probs.push("topology");
      if (d.features.length !== native.n_features) {
        probs.push(`features ${d.features.length} vs ${native.n_features}`);
      }
      if (d.primers.length !== native.n_primers) {
        probs.push(`primers ${d.primers.length} vs ${native.n_primers}`);
      }
      if (d.lowercase !== native.lowercase) {
        probs.push(`lowercase ${d.lowercase} vs ${native.lowercase}`);
      }
      if (probs.length) mismatches.push(`${path.basename(f).slice(0, 40)}: ${probs.join("; ")}`);
    }
    ok++;
  }

  if (dnaFiles.length) {
    T(".dna files read through the page", bad === 0,
      `${ok}/${dnaFiles.length} files, ${feats} features, ${bases.toLocaleString()} bases`);
    if (plBin) {
      T("page agrees with the native binary", mismatches.length === 0,
        mismatches.length ? mismatches.slice(0, 3).join(" | ") : `${ok} files`);
    }
  } else {
    console.log("  (no .dna corpus given, skipped)");
  }

  const gbFiles = expand(process.argv[3], [".gb", ".gbk", ".genbank"]).slice(0, 300);
  let gbOk = 0, gbBad = 0, gbFeat = 0;
  for (const f of gbFiles) {
    window.__bytes = new Uint8Array(fs.readFileSync(f));
    try {
      const d = F("coreOpen(new Uint8Array(window.__bytes))");
      gbFeat += d.features.length;
      gbOk++;
    } catch (e) {
      gbBad++;
      if (gbBad <= 3) console.log(`    ${path.basename(f).slice(0, 44)}: ${e.message}`);
    }
  }
  if (gbFiles.length) {
    T("GenBank files read through the page", gbBad === 0,
      `${gbOk}/${gbFiles.length} files, ${gbFeat.toLocaleString()} features`);
  }

  console.log("\n=== still no uncaught errors ===");
  T("clean console", errors.length === 0, errors.slice(0, 3).join(" | "));

  console.log(`\n${failures === 0 ? "ALL CHECKS PASSED" : failures + " CHECK(S) FAILED"}`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch(e => {
  console.error("harness error:", e);
  process.exit(1);
});
