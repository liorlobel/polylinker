# Security

Polylinker ships an Ed25519 public key compiled into two of its binaries, and
code in those binaries that decides what to keep based on that key. That is a
trust anchor, and a project with a trust anchor and no way to report a problem
with it is worse than one with neither. This file is the way.

It is also the honest account of what that key does and does not buy, and of
what happens if the private half of it leaks. The short version of the second
one: it would be bad, there is no revocation channel, and the recovery is
manual. The detail is in [If the release key is
compromised](#if-the-release-key-is-compromised), which is the part of this file
worth reading before the rest.

## Reporting

**Use GitHub private vulnerability reporting.** Go to
<https://github.com/liorlobel/polylinker/security> and choose *Report a
vulnerability*. It is private, it is authenticated, and it produces a thread
that cannot be lost in an inbox.

There is deliberately no security email address in this file. An address
published in a public repository collects spam until it is unusable, and it is
one more thing that has to keep working — a forwarding rule that survives
somebody changing university, a mailbox somebody remembers to read. The GitHub
form needs none of that.

**Do not open a public issue for anything in the scope list below.** Everything
else — a wrong restriction digest, a crash on a file you can attach, a parser
that mangles a qualifier — belongs in the public tracker, and those are welcome
there.

### What to expect

Polylinker is maintained by one person, on academic time, alongside running a
lab. There is no security team, no rota, and no out-of-hours anything. So rather
than a service level nobody would meet:

* **Acknowledgement: within a week**, meaning a reply that says the report was
  read and whether it is understood. Term time and conference travel are the
  usual reasons it takes the whole week.
* **An assessment — is it real, how bad, what is the fix — within a month.**
* **A fix ships in the next release.** For anything touching signature
  verification or the update path that release is cut immediately rather than
  waited for.

If two weeks pass with nothing, open a public issue saying only that a private
report is waiting and giving its date. No details — that is a nudge, not a
disclosure, and it is a reasonable thing to do rather than a rude one.

### Disclosure

Coordinated, and not indefinitely. Please hold the details until a fixed release
exists; if 90 days pass and no fix has shipped, publish. Sitting on a live
vulnerability in somebody else's tool is not something this project will ask
anyone to do, and a maintainer who has gone quiet has forfeited the right to
ask.

Reporters are credited by name in the release notes unless they would rather
not. Say which.

There is no bug bounty. There is no money in this project at all — the reason
the builds are unsigned is that a code-signing certificate is unfunded.

## Supported versions

**The latest release only.** Fixes go into a new release; there is no backport
branch, no LTS, and no separate security-only channel — a second release path
would be a second set of things to get wrong, maintained by the same one person.
The current release is listed at
<https://github.com/liorlobel/polylinker/releases>, and `pl --version` prints
what a given copy is.

Everything before it is superseded the moment it is superseded, including for
security fixes. If you are running an older copy and it matters, the answer is
to install the current one.

## In scope

Concretely, in rough order of how much a report would matter:

**`crates/pl-core/src/ed25519.rs` — the signature verifier.** Hand-written,
dependency-free, and the thing every other guarantee here stands on. Its module
doc lists what it must refuse — `S >= L`, non-canonical point encodings, points
off the curve, `x = 0` with the sign bit set, small-order public keys — and
states that nothing in the file panics on any input. **A way past any item on
that list, or any input that panics, is the highest-value report this repository
can receive.** So is a disagreement with libsodium or OpenSSL in the accepting
direction: the file implements the cofactorless equation on purpose, so anything
it accepts should be accepted by those too.

**`crates/pl-update` — the update path.** `docs/RELEASING.md` sets four
requirements and the crate claims to meet all four. Anything that breaks one of
them is in scope: a path on which bytes are kept without the manifest signature
having verified; a path on which the artifact is requested before the signature
is checked; a manifest reachable other than through `VerifiedManifest::verify`;
a downloaded file kept when its SHA-256 does not match the entry in the verified
manifest; a write into the directory the running binary lives in; anything that
runs without being called. Also in scope: `pl_update` becoming reachable from
anywhere other than `cmd_update` in `bins/pl` and `update.rs` in `bins/pl-gui`,
and anything that makes the desktop app's update check run when the setting is
off or a settings file is damaged.

**The file parsers — `crates/pl-fileio` (`.dna`, GenBank, FASTA) and
`crates/pl-abif` (`.ab1`).** These read attacker-supplied input, and that is not
a hypothetical framing: a plasmid map arrives as an email attachment from a
collaborator, off a shared drive, or out of a repository, and the whole point of
the tool is that you open it without thinking about it. Both crates are safe
Rust with no `unsafe` anywhere in them, so memory corruption is not the expected
failure mode. What is: a panic, a hang, or an allocation sized from a number
read out of the file. A crafted file that makes any of the parsers do one of
those is a bug worth reporting privately, and so is one that causes a write
outside the directory `pl convert -o` was pointed at.

A crafted file that is read back as a *different molecule* than it is — the
right length, the right name, silently the wrong sequence or the wrong feature
coordinates — is in scope too, and for this tool it is arguably worse than a
crash. Ordinary wrong answers on ordinary files are not security reports; they
are the public tracker, and they are taken just as seriously there.

**`crates/pl-wasm` — the browser build.** The one crate with an `unsafe` ABI:
`pl_open`, `pl_to_genbank`, `pl_to_fasta` and the rest take a pointer and a
length from the host. The module declares zero imports and so cannot reach the
network, and `prototype/dna-reader.html` is one self-contained file that runs
over `file://`. A way to make that ABI read or write outside the buffer it was
handed is in scope.

**The installer — `tools/installer/`, the MSI built from it, and
`Install-Polylinker.ps1`.** In scope: writing outside
`%LOCALAPPDATA%\Programs\Polylinker` or `C:\Program Files\Polylinker`; a path
that elevates when the per-user install is not supposed to; taking a file
association away from a program the user already has; an uninstall that removes
anything under `%LOCALAPPDATA%\Polylinker\recovery`, which holds unsaved work
rescued from a crash. Also anything that reaches the network or schedules work —
`tools/ci.ps1` fails the build if a network or scheduling facility appears
anywhere under `tools/installer/`, so a way past that check is itself a report.

**The release pipeline — `.github/workflows/release.yml`, `tools/release.ps1`,
`tools/build-msi.ps1`, `tools/ci.ps1`.** In scope: getting a file into a
published archive that is not in `dist/`; getting the signing step to sign
something other than what is published; getting `POLYLINKER_RELEASE_KEY` or
anything derived from it into a log; defeating the step that verifies the
signature against the public key read out of `crates/pl-update/src/lib.rs`, or
the negative control that proves that verification can fail.

**`bins/pl-mcp` — the MCP server.** Read-only by construction, so that an
assistant can ask about a plasmid without being able to overwrite one. A path by
which it writes anything, or reads outside what it was pointed at, is in scope.

## Out of scope

Not because they do not matter — several of them matter a great deal — but
because they are known, documented, and not news:

* **The builds are unsigned.** There is no code-signing certificate and no Apple
  Developer ID, so Windows SmartScreen and macOS Gatekeeper do not recognise the
  publisher and say so. This is a funding problem, `docs/RELEASING.md` sets out
  what it costs and who would have to buy it, and a report that Windows shows a
  warning is a report that the documentation is accurate.
* **There is no revocation channel for the release key.** Documented below and
  in `docs/RELEASING.md:188`. The section below is the answer, such as it is.
* **`pl update` and the desktop app's update check reveal an IP address to
  github.com.** They send no sequence, no file name and no identifier, the CLI
  form only runs when somebody types the verb, and the desktop switch ships off.
  That trade is stated beside the checkbox before anyone can agree to it.
* **A machine already under someone else's control.** Nothing in a local editor
  survives that, and this one deliberately keeps unsaved work in
  `%LOCALAPPDATA%\Polylinker\recovery` in the clear so that a user can find it
  after a crash.
* **Vulnerabilities in other software** Polylinker reads files from, writes
  files for, or is tested against — SnapGene, Biopython, pydna, ApE, Benchling.
  Report those to them.
* **Scanner output with no reachable path.** `pl-core` and the crates behind the
  CLI have no external dependencies at all; the exceptions are `eframe`, `rfd`
  and `egui-phosphor` in the desktop app, `pyo3` in the Python bindings, and the
  system `curl`, which is the operating system's and is patched by it. An
  advisory in one of those five *with an argument for how Polylinker reaches
  it* is welcome. A `cargo audit` transcript is not.

**There is no infrastructure to test.** No server, no hosted service, no
account, no telemetry endpoint, no login. Nothing is being offered here for
anyone to probe, and nothing needs permission to be granted for it.

## Threat model

The assumption is that the machine and its operating system are trusted, and
that **files and the network are not**. Everything above follows from that.

### What the manifest signature proves

`SHA256SUMS.txt.sig` is 64 raw bytes: an Ed25519 signature over
`SHA256SUMS.txt`, made with the release key. It proves that **the checksum table
came from whoever holds that key**. That is a statement about origin, and it is
the one an updater needs, because an attacker who controls the server controls
the checksums too — a checksum fetched from the same place as the file it
describes proves only that the download completed.

The archives are covered transitively rather than directly: the manifest names
each file and its SHA-256, so verifying the signature and then re-hashing the
file on disk is a chain from the compiled-in key to those bytes. Both links have
to be checked. Verifying the signature and forgetting to re-hash the download
verifies nothing about the download.

### What it does not prove

* **It is not code signing.** Windows and macOS have never heard of this key and
  never will; it lives in a Rust constant, not in a certificate any operating
  system trusts. It buys nothing at the SmartScreen dialog and everything after
  it.
* **It does not say a release is good**, only that it was made by whoever holds
  the private half.
* **It does not survive compromise of that private half.** See below.
* **It protects `pl` and `polylinker` only.** The key is
  `pl_update::RELEASE_PUBLIC_KEY`, and only those two binaries depend on
  `pl-update`. `bins/pl-mcp`, `crates/pl-py` and `crates/pl-wasm` do not carry
  the key or any code that could use it, because none of them can update
  anything. Prose here that says "every binary" would be wrong.

### The one hop that is not verified, and what it costs

`pl_update::check` reads a version number out of the *latest* release's
manifest. That text cannot be verified: it is fetched before anyone knows which
release is being talked about, and it is the thing that says which release that
is. So it is a claim by whoever answers on that hostname.

What an attacker who controls that hostname gets from it is a lie about a
version number, and nothing else. The claim can only ever become a `Version` —
three integers — so it cannot put a path or a flag into any URL built from it.
`fetch_and_verify` then fetches the manifest again, from the specific version's
URL rather than the `latest` redirect, and *that* copy is the one whose
signature is checked and whose digests are used. A claim pointing at an older
release is refused outright, because an old release is genuinely signed and a
rollback would otherwise verify perfectly. Nothing unsigned is ever kept, and
nothing is ever executed: the last thing the crate does is return the path of a
verified file for a person to run, and it refuses to write into the directory
the running binary is in.

### The first copy

Trust flows backwards through the install history: version *n* vouches for
version *n+1*. The very first copy is vouched for by however you got it — a
checksum compared by hand, a package manager, a colleague's USB stick. **Nothing
here improves that first hop and nothing can.** Every release page prints the
OpenSSL command to check the signature by hand, which is the one thing that does
not depend on already trusting a copy of Polylinker.

### What is sent

Nothing, by default. The desktop app contacts nothing unless the update check
under Help is switched on, and it ships off; a damaged or hand-edited settings
file falls back to off rather than on. Switched on, it asks github.com once per
launch whether a newer release exists and never downloads anything. `pl update`
makes a request because somebody typed the verb. The installer contacts nothing
at all. No sequence, no file name and no identifier leaves the machine on any of
these paths; the request tells github.com an IP address and nothing about the
work.

## If the release key is compromised

This is the worst thing that can happen to this project, and it is worth being
blunt about why: a parser bug reaches one person's file, and a forged release
reaches everybody's next update.

### The key

```
hex     5a53cfdab24df9b4d8e918aed8e03338bdcac10b073a6f59d21d3ee9836be3b7
base64  WlPP2rJN+bTY6Riu2OAzOL3KwQsHOm9Z0h0+6YNr47c=
```

An Ed25519 public key is 32 bytes, so that is the key itself rather than a
fingerprint of it — there is nothing shorter to compare against. It is
`pl_update::RELEASE_PUBLIC_KEY` in `crates/pl-update/src/lib.rs`, in three
spellings that `crates/pl-update/tests/key.rs` requires to agree, compiled into
`pl` and `polylinker`.

### Who can sign

The private half is a GitHub Actions secret named `POLYLINKER_RELEASE_KEY` on
`liorlobel/polylinker`, holding the base64 of its 32 raw bytes. It is on no
developer machine and in no file in this repository, and
`crates/pl-core/src/ed25519.rs` verifies but deliberately cannot sign — adding a
signing routine to that file would be a bug, for reasons its own module doc
gives at length.

Which means the set of parties who can produce a release that every installed
copy of `pl` and `polylinker` will accept is: **anyone who can push to this
repository, anyone who can read that secret, and GitHub.** That is one
maintainer's account, whatever GitHub Actions runs on this repository, and the
platform itself. It is a small set and it is not a hard one to state, so it is
stated.

### There is no revocation channel

`docs/RELEASING.md:188` records this and it is not softened here. Installed
copies trust the compiled-in key, and there is no mechanism by which they can be
told to stop — because such a mechanism would be a network call, and the whole
design is that there is not one. Rotating the key therefore means editing the
constant, cutting a release, **and every user installing that release by hand.**
Until a user does that, their copy goes on accepting anything the old key
signed.

That is a real cost of the offline guarantee, it was accepted knowingly, and
nothing below fixes it. What follows is damage control.

### What would be done

Untested. This procedure has never been executed, so read it as a plan rather
than as a drill that has been run.

1. **Revoke first.** Delete `POLYLINKER_RELEASE_KEY` from the repository's
   secrets and disable Actions on the repository, so that nothing can produce a
   signature while the rest of this is happening. If the maintainer's GitHub
   account is the suspected route in, reset that account first — a secret
   deleted by somebody who is still inside the account is not deleted.
2. **Establish what was signed.** Every release is on the releases page with its
   `SHA256SUMS.txt`. Rebuild each from the tag it names and compare. The thing
   being looked for is a release that exists on that page and corresponds to no
   tag in this repository, or one whose manifest does not match a rebuild of its
   own tag.
3. **Mint a new key, offline, on a machine that is not the compromised one.**
   `openssl genpkey -algorithm ed25519`; the public half is the last 32 bytes of
   the DER `SubjectPublicKeyInfo`. It goes into the three constants in
   `crates/pl-update/src/lib.rs`, and its private half — base64 of the 32 *raw*
   bytes, not hex, not a PEM, not a PKCS#8 DER — into a new repository secret.
   `release.yml` derives the public half of whatever the secret holds and fails
   the release if it does not equal the constant in the source, so the two
   cannot be rotated apart by accident.
4. **Cut a release from a commit that has been read**, not merely from one that
   is green. A gate proves properties of code; it does not prove that the code
   is the code that was intended.
5. **Say so everywhere that will be read before somebody's next update**: a
   release whose notes lead with it, the README status block, this file, and a
   pinned public issue. Name the compromised key by its hex, name the
   replacement, give the date range within which a forged signature could exist,
   and state plainly that an installed copy will go on accepting the old key
   until it is replaced by hand. It will.
6. **Do not delete the compromised releases.** Mark them. Deleting them destroys
   the evidence of what was published and breaks every checksum anyone recorded
   against them; what has to be taken away is the impression that they are safe
   to install, not the record that they existed.

Anyone installing during or after such an event should check the download
against the new key by hand, with the OpenSSL command in the release notes,
rather than with `pl update` — the copy already on the machine may be the
compromised one, and a compromised binary is not a witness to its own
replacement.

### What would actually fix it

There is a standard answer and it is not built: an offline root key, kept
somewhere that is not a CI secret, compiled into the binaries, whose only job is
to sign release keys — so a rotation is a signed statement that existing
installs can check for themselves rather than a request that every user reinstall
by hand. It is the right shape and it is not here, because it needs a second key
managed with real discipline over years by one person who does not currently
have anywhere to keep it, and getting that wrong quietly would be worse than not
claiming it. Until it exists, the guarantee this project offers is exactly the
one described above and no more.
