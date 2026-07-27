# Releasing

## What is done, and what is not

`tools/release.ps1` builds the binaries, records the commit and toolchain, and
writes `SHA256SUMS.txt`. It signs if given an identity and says loudly that it
did not if it was not.

**The builds are unsigned, and I cannot change that.** Signing needs
credentials that are issued to a person or an organisation:

| Platform | What is needed | Roughly | Who must obtain it |
|---|---|---|---|
| Windows | An OV or EV code-signing certificate. EV or an [Azure Trusted Signing] subscription is what actually clears SmartScreen quickly; an OV certificate builds reputation slowly | £200–400/yr, or ~$120/yr for Azure Trusted Signing | Lior, or Bar-Ilan |
| macOS | Apple Developer Program membership, a *Developer ID Application* certificate, and an app-specific password for `notarytool` | $99/yr | Lior |
| Linux | Nothing. A `.tar.gz` with a checksum is the norm | — | — |

[Azure Trusted Signing]: https://learn.microsoft.com/azure/trusted-signing/

Until then, `SHA256SUMS.txt` is the integrity guarantee. Publish it beside the
binaries. It is weaker than a signature — it proves the file matches what the
release page says, not who built it — and saying which of the two you have is
the point.

### What an unsigned build costs the user

Windows SmartScreen shows "Windows protected your PC" on first run and needs
*More info → Run anyway*. macOS Gatekeeper refuses outright and needs a
right-click → Open, or a trip to System Settings. Neither is fatal; both look
exactly like what malware looks like, which is the real cost. An academic tool
asking a labmate to click past a security warning is teaching a bad habit.

## macOS notarisation

Must run on macOS, so it is not in `release.ps1`:

```bash
codesign --force --options runtime --timestamp \
  --sign "Developer ID Application: NAME (TEAMID)" dist/polylinker
ditto -c -k --keepParent dist/polylinker dist/polylinker.zip
xcrun notarytool submit dist/polylinker.zip \
  --apple-id APPLE_ID --team-id TEAMID --password APP_SPECIFIC_PASSWORD --wait
xcrun stapler staple dist/polylinker
```

`--options runtime` is not optional: notarisation rejects a binary without the
hardened runtime, and the rejection message does not say so clearly.

## There is no auto-updater, on purpose

This was a decision, not an omission.

Polylinker's claim is that it runs offline and sends nothing anywhere. An
auto-updater contradicts that twice over: it phones a server on a schedule,
which is a beacon saying this machine exists and is running this version, and it
downloads and executes code the user did not ask for. On a lab machine that also
holds unpublished sequence, both are worth avoiding.

The update path is therefore: **the user checks when the user wants to.**
`pl --version` prints the version and the commit. The release page lists the
current one. That is the whole mechanism.

If an updater is ever added, the bar it has to clear is written down here so the
question is not reopened casually:

1. It downloads nothing without being asked, each time.
2. It verifies a signature over the download before the bytes touch disk in an
   executable location — a checksum fetched from the same server as the file
   proves nothing about an attacker who controls the server.
3. The public key is compiled into the binary being replaced, so the trust
   anchor is not fetched from the network.
4. It never replaces a running binary silently.

Any updater that cannot meet all four is worse than telling the user to
download the new version themselves.

## Reproducibility

`SHA256SUMS.txt` records the commit and the exact `rustc` version, and the
script warns when the working tree is dirty, because a hash that cannot be tied
to a commit is a number and not a guarantee. Byte-for-byte reproducible builds
across machines are **not** claimed: the build embeds absolute paths, and
verifying the claim needs a second machine to build on. Saying so is better than
implying a property nobody has checked.
