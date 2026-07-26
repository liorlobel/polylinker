/**
 * Headless validation of dna-reader.html.
 *
 * Runs the real page in a real DOM, then drives its own parsers and writers
 * over real files from disk. Checks that:
 *   - the page loads and renders without throwing
 *   - format sniffing picks the right codec from content, not extension
 *   - .dna -> GenBank keeps sequence, topology, features and coordinates
 *   - the GenBank it writes can be read back by its own parser
 *   - SVG export resolves CSS variables (no `var(...)` left in the output)
 *
 * Usage:  node check_page.js [glob-of-dna-files] [glob-of-genbank-files]
 */
const fs = require("fs");
const path = require("path");
const { JSDOM, VirtualConsole } = require("jsdom");

const HTML = path.join(__dirname, "dna-reader.html");
const src = fs.readFileSync(HTML, "utf8");

const errors = [];
const vc = new VirtualConsole()
  .on("jsdomError", e => errors.push("jsdomError: " + (e.message || e)))
  .on("error", (...a) => errors.push("console.error: " + a.join(" ")));

const dom = new JSDOM(`<!doctype html><html><head></head><body>${src}</body></html>`, {
  runScripts: "dangerously", pretendToBeVisual: true, virtualConsole: vc,
});
const { window } = dom;
const doc = window.document;

let failures = 0;
function T(label, cond, extra = "") {
  console.log(`${cond ? "  PASS" : "  FAIL"}  ${label}${extra ? "  — " + extra : ""}`);
  if (!cond) failures++;
  return cond;
}

function expand(pattern) {
  if (!pattern) return [];
  // minimal ** glob without pulling in a dependency
  const star = pattern.indexOf("*");
  if (star === -1) return fs.existsSync(pattern) ? [pattern] : [];
  const root = pattern.slice(0, pattern.lastIndexOf(path.sep, star));
  const ext = path.extname(pattern);
  const out = [];
  (function walk(d, depth) {
    if (depth > 8) return;
    let entries;
    try { entries = fs.readdirSync(d, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p, depth + 1);
      else if (!ext || e.name.toLowerCase().endsWith(ext.toLowerCase())) out.push(p);
    }
  })(root, 0);
  return out.sort();
}

setTimeout(() => {
  const F = window.eval.bind(window);

  console.log("=== page load ===");
  T("no uncaught errors", errors.length === 0, errors.slice(0, 2).join(" | "));
  T("summary rendered", /circular/.test(doc.getElementById("summary").textContent));
  T("map drawn", doc.getElementById("mapHost").querySelectorAll("path").length >= 8);
  T("no NaN in geometry", !doc.getElementById("mapHost").innerHTML.includes("NaN"));

  console.log("\n=== export buttons present ===");
  for (const id of ["expGb", "expFa", "expSvg", "dlBtn", "openBtn"]) {
    T(`#${id}`, !!doc.getElementById(id));
  }

  console.log("\n=== SVG export ===");
  const svg = F("mapToSvg()");
  T("produces svg", /^<\?xml/.test(svg) && svg.includes("<svg"), `${svg.length} chars`);
  T("CSS variables resolved", !svg.includes("var(--"),
    (svg.match(/var\(--[a-z-]+\)/) || ["none left"])[0]);
  T("has background rect", svg.includes("<rect"));

  console.log("\n=== GenBank round-trip on the demo construct ===");
  const gb = F(`toGenBank(CURRENT, "demo-construct.dna")`);
  T("LOCUS line well formed", /^LOCUS {7}\S+ +\d+ bp {4}DNA {5}circular SYN \d{2}-[A-Z]{3}-\d{4}/.test(gb),
    gb.split("\n")[0]);
  T("ends with //", gb.trimEnd().endsWith("//"));
  const back = F(`parseGenBank(${JSON.stringify(gb)})`);
  const orig = F("CURRENT");
  T("sequence survives", back.sequence === orig.sequence,
    `${back.sequence.length} vs ${orig.sequence.length} bp`);
  T("topology survives", back.circular === orig.circular);
  T("feature count survives", back.features.length === orig.features.length,
    `${back.features.length} vs ${orig.features.length}`);
  T("colours survive", back.features.every(f => /^#[0-9a-f]{6}$/i.test(f.color || "")),
    back.features.map(f => f.color).slice(0, 3).join(","));
  const coordsOk = back.features.every((f, i) =>
    f.start === orig.features[i].start && f.end === orig.features[i].end);
  T("coordinates survive", coordsOk);
  // GenBank cannot express "unoriented": a feature is either on the forward
  // strand or wrapped in complement(). SnapGene's dir=0 therefore becomes
  // forward on round-trip. That is a documented, unavoidable loss.
  const strandOk = back.features.length === orig.features.length &&
    back.features.every((f, i) => {
      const want = orig.features[i].dir;
      return f.dir === want || (want === 0 && f.dir === 1);
    });
  T("strand survives (dir=0 -> forward, a GenBank limitation)", strandOk,
    strandOk ? "" : back.features
      .map((f, i) => `${f.dir}/${orig.features[i] ? orig.features[i].dir : "-"}`).join(" "));

  console.log("\n=== real .dna corpus ===");
  const dnaFiles = expand(process.argv[2]).filter(f => f.toLowerCase().endsWith(".dna"));
  let dnaOk = 0, dnaBad = 0, totalFeat = 0;
  for (const f of dnaFiles) {
    try {
      const buf = fs.readFileSync(f);
      const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength);
      const bytes = new Uint8Array(ab);
      const head = Buffer.from(bytes.subarray(0, 4000)).toString("utf8");
      const fmt = F("sniffFormat")(bytes, head);
      if (fmt !== "dna") throw new Error(`sniffed as ${fmt}`);
      const d = F("parseDna")(ab);
      const g = F("toGenBank")(d, path.basename(f));
      const b = F("parseGenBank")(g);
      if (b.sequence !== d.sequence) throw new Error("sequence lost via GenBank");
      // GenBank has no separate primer object: each binding site is written as
      // a primer_bind feature and reads back as one. Account for that.
      const nSites = d.primers.reduce((n, p) => n + p.sites.filter(s =>
        s.start >= 1 && s.end <= d.length && s.end >= s.start).length, 0);
      const want = d.features.length + nSites;
      if (b.features.length !== want) {
        throw new Error(`features ${b.features.length} != ${want} (${d.features.length} + ${nSites} primer sites)`);
      }
      if (b.circular !== d.circular) throw new Error("topology lost");
      totalFeat += d.features.length;
      dnaOk++;
    } catch (e) {
      dnaBad++;
      if (dnaBad <= 4) console.log(`    ${path.basename(f).slice(0, 44)}: ${e.message}`);
    }
  }
  if (dnaFiles.length) {
    T(`.dna parse + GenBank round-trip`, dnaBad === 0,
      `${dnaOk}/${dnaFiles.length} files, ${totalFeat} features`);
  } else {
    console.log("  (no .dna corpus given, skipped)");
  }

  console.log("\n=== real GenBank corpus ===");
  const gbFiles = expand(process.argv[3]).filter(f => /\.(gb|gbk|genbank)$/i.test(f));
  let gbOk = 0, gbBad = 0, gbFeat = 0, gbEmpty = 0;
  for (const f of gbFiles.slice(0, 300)) {
    try {
      const text = fs.readFileSync(f, "utf8");
      const bytes = new Uint8Array(Buffer.from(text.slice(0, 4000)));
      if (F("sniffFormat")(bytes, text.slice(0, 4000)) !== "genbank") throw new Error("not sniffed as genbank");
      const d = F("parseGenBank")(text);
      if (!d.sequence.length) { gbEmpty++; throw new Error("no sequence parsed"); }
      gbFeat += d.features.length;
      gbOk++;
    } catch (e) {
      gbBad++;
      if (gbBad <= 4) console.log(`    ${path.basename(f).slice(0, 44)}: ${e.message}`);
    }
  }
  if (gbFiles.length) {
    T("GenBank files parsed", gbBad === 0,
      `${gbOk}/${Math.min(gbFiles.length, 300)} files, ${gbFeat} features total`);
  } else {
    console.log("  (no GenBank corpus given, skipped)");
  }

  console.log(`\n${failures === 0 ? "ALL CHECKS PASSED" : failures + " CHECK(S) FAILED"}`);
  process.exit(failures === 0 ? 0 : 1);
}, 700);
