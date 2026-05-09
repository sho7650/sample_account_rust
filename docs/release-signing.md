# Release signing — maintainer runbook

This document is the operational reference for the signing chain in
`.github/workflows/release.yml`. End-user *verification* commands live
in the README's "Verifying release artifacts" section.

## What gets signed

| Target | What | Tool | Cost | Status |
|---|---|---|---|---|
| **Linux x86_64** | The `.tar.gz` archive | cosign keyless (sigstore OIDC) | $0 | Active |
| **macOS aarch64** | The Mach-O binary, then the `.tar.gz` archive | Apple Developer ID Application + Hardened Runtime + `xcrun notarytool`, then cosign keyless. **No stapling** — see below. | Apple Developer Program: **$99 / yr** | Active |
| **Windows x86_64** | The `.zip` archive only (Authenticode of the .exe is deferred — see below) | cosign keyless | $0 | Active (cosign only) |

Total recurring cash cost: **$99 / yr** (Apple). Apple cert is valid
5 years; the Apple Developer Program membership renews annually.

### Why Windows Authenticode is deferred

SignPath.io is the obvious zero-cost Authenticode path for an OSS
project, but their **Free Trial tier does not support Trusted Build
Systems** — the OIDC-based origin verification that lets SignPath
confirm a signing request actually came from this repository's CI
rather than from a leaked API token.

Without that mechanism the API token becomes the sole authentication
factor, which is below the security bar we want for release signing.
The Authenticode steps are therefore commented out in `release.yml`
and end users will continue to see the SmartScreen warning on the
Windows binary until the Foundation OSS tier is approved (or an
alternative path like Azure Trusted Signing is adopted).

Provenance is still cryptographically verifiable on Windows via
cosign — the missing piece is purely OS-level reputation/UX.

### Why macOS notarization is NOT stapled

Apple's `xcrun stapler` writes the notarization ticket into the
extended attributes / resource fork of a **container**: `.app`
bundles, `.dmg` disk images, `.pkg` installer packages, `.kext`,
`.dext`. **A raw Mach-O command-line binary has no such container**,
so there is nowhere to put the ticket. Calling stapler on one fails
with exit 73 / "CloudKit returned error 4096" — the message is
misleading; it is not a propagation issue.

This is documented behavior, not a bug. Authoritative sources:

- **Apple Developer Forums / docs**: stapling is *optional*. From
  Apple's "Customizing the Notarization Workflow" page (paraphrased
  by community members and Apple staff replies): if you choose to
  skip stapling, your software still functions — users' systems
  verify notarization with Apple's servers instead of using a
  locally stapled ticket.
- **Rob Allen, "Notarising a macOS standalone binary"**
  ([akrabat.com](https://akrabat.com/notarising-a-macos-standalone-binary/)):
  > "for a standalone binary, _this cannot be done_ as there is no
  > directory into which to store the notarisation file."
  > "For standalone binaries GateKeeper checks directly with Apple's
  > servers the first time they are run, so stapling is unnecessary."
- **Random Errata, "A very rough guide to notarizing CLI apps for
  macOS" (2024)**
  ([randomerrata.com](https://www.randomerrata.com/articles/2024/notarize/)):
  distributes as `.tar.gz`, accepts the trade-off. Notes a `.dmg`
  would be "nicer" because it could be stapled for offline
  verification, but doesn't implement that.
- **ScriptingOSX, "Notarize a Command Line Tool with notarytool"**
  ([scriptingosx.com](https://scriptingosx.com/2021/07/notarize-a-command-line-tool-with-notarytool/)):
  > "Command Line Tools can be signed, but not directly notarized.
  > You can however notarize a pkg file containing the Command Line
  > Tool. Also, it is much easier for users and administrators to
  > install your tool when it comes in a proper installation
  > package."

We therefore: codesign the binary, submit to notarytool (status:
Accepted), and **stop there**. The notarization record exists in
Apple's database — Gatekeeper will fetch and verify it at first
launch on the user's Mac.

#### End-user implication

- **Online at first launch (typical case)** — Gatekeeper queries
  Apple to confirm notarization, caches the result locally, and the
  binary runs with no popup, no `xattr` step. All subsequent runs
  work offline.
- **Completely offline at first launch (uncommon)** — Gatekeeper
  cannot reach Apple, may refuse to launch the binary or show a
  warning. The user can: connect to a network and re-launch; OR
  manually trust via System Settings → Privacy & Security; OR run
  `xattr -d com.apple.quarantine sample_account` as a one-time
  override.

Same trade-off adopted by ripgrep, bat, uv, and other major Rust
CLI tools.

#### Future option: `.pkg` distribution

If demand for offline-first-launch verification arises, the
upgrade path is:

1. After codesign + notarize-and-staple-skip, wrap the signed Mach-O
   in a `.pkg` via `productbuild --component sample_account /usr/local/bin sample_account.pkg`.
2. codesign the `.pkg` with the Developer ID Installer certificate
   (a separate cert from Developer ID Application — needs to be
   added to the Apple Developer portal and the GitHub Secrets).
3. Submit the `.pkg` to notarytool, wait for Accepted.
4. `xcrun stapler staple sample_account.pkg` — this works because
   `.pkg` is a stapleable container.
5. Distribute the `.pkg` as the macOS asset instead of `.tar.gz`.

This is deferred — current users seem fine with the online-first-
launch model.

### Re-enabling Windows Authenticode

When SignPath OSS is approved (or when migrating to Azure Trusted
Signing), restore these 4 GitHub Secrets to the `release-signing`
environment:

| Secret | Notes |
|---|---|
| `SIGNPATH_API_TOKEN` | scoped to the production signing policy |
| `SIGNPATH_ORGANIZATION_ID` | UUID |
| `SIGNPATH_PROJECT_SLUG` | URL-friendly project slug (e.g. `sample_account`) |
| `SIGNPATH_SIGNING_POLICY_SLUG` | URL-friendly policy slug (e.g. `CI_Resease`) |

…and re-add the 5 Windows steps removed in commit
[TBD: link to the deferral commit] to `release.yml`. The signing
policy in SignPath must have a `GitHub Actions` Trusted Build
System configured against `sho7650/sample_account_rust` /
`.github/workflows/release.yml` / `refs/tags/v*` before any signing
request will succeed.

## Why these tools

- **cosign keyless** uses the GitHub Actions OIDC token to obtain a
  short-lived (~10 min) signing certificate from Sigstore Fulcio. The
  signature + cert + Rekor inclusion proof are bundled into a single
  `.cosign.bundle` file that users can verify offline indefinitely.
  Costs nothing, no key material to rotate, signature provenance is
  cryptographically tied to the GitHub repository identity.
- **Apple Developer ID + notarization + staple** is the only path to a
  macOS binary that runs on a downloaded clean Mac without prompts.
  Ad-hoc signing is not enough for downloaded binaries (Gatekeeper).
- **SignPath.io** is a HSM-backed managed Authenticode signing service
  with a free tier for OSS projects. The alternative (EV cert + USB
  hardware token) is incompatible with cloud CI without paid HSM
  proxies; SignPath sidesteps that entirely.

## Required GitHub Secrets

All 6 secrets MUST live in a single GitHub environment named
`release-signing` (Repository → Settings → Environments → New
environment). Repository-level secrets are visible to too many
workflows; environment-level secrets are scoped to the job that
declares `environment: release-signing` AND can be gated on a
required-reviewer policy.

| Secret | Source | Notes |
|---|---|---|
| `MACOS_DEVELOPER_ID_P12_BASE64` | Developer ID Application certificate exported from Keychain Access as `.p12`, then `base64 -i Cert.p12 \| pbcopy`. | Base64-encoded so it round-trips through GitHub Secrets without binary mangling. |
| `MACOS_DEVELOPER_ID_P12_PASSWORD` | The password set at `.p12` export time. | Used to import the key into the temp keychain inside CI. |
| `MACOS_DEVELOPER_ID_IDENTITY` | Output of `security find-identity -v -p codesigning` on the Mac that exported the cert. The full string, e.g. `Developer ID Application: Yamane Sho (ABCDE12345)`. | Passed verbatim to `codesign --sign`. |
| `MACOS_NOTARY_API_KEY_BASE64` | App Store Connect → Users and Access → Integrations → App Store Connect API → "+" → "Developer" role → download `AuthKey_XXXXXXXXXX.p8` (one-time download), then `base64 -i AuthKey_*.p8 \| pbcopy`. | The .p8 is downloadable exactly once. Store the original somewhere durable as well. |
| `MACOS_NOTARY_API_KEY_ID` | Shown in App Store Connect next to the key, e.g. `ABCDE12345`. | 10-char alphanumeric. |
| `MACOS_NOTARY_API_ISSUER_ID` | Shown at the top of the App Store Connect API page. | UUID format. |

The 4 SignPath secrets needed to re-enable Windows Authenticode
signing are documented above under
"[Re-enabling Windows Authenticode](#re-enabling-windows-authenticode)".

## One-time setup

### Apple Developer + notarization

1. Enroll in the Apple Developer Program ($99/yr individual; longer
   lead time + D-U-N-S for organizations). Lead time: 1-7 days.
2. In the Apple Developer portal: Certificates → +  → Developer ID
   Application. Generate a CSR from Keychain Access, upload, download
   the resulting cert, double-click to add to your login keychain.
3. In Keychain Access: select the imported cert and its private key,
   right-click → Export 2 items → format `.p12`, set a strong password.
   This becomes `MACOS_DEVELOPER_ID_P12_BASE64` (after base64) and
   `MACOS_DEVELOPER_ID_P12_PASSWORD`.
4. `security find-identity -v -p codesigning` → copy the full
   `Developer ID Application: …` string into
   `MACOS_DEVELOPER_ID_IDENTITY`.
5. App Store Connect → Users and Access → Integrations → App Store
   Connect API → request access (one-time) → "+" → name the key,
   role `Developer` → download the `.p8` (only available at creation).
   Note Key ID + Issuer ID. These become `MACOS_NOTARY_API_KEY_BASE64`,
   `MACOS_NOTARY_API_KEY_ID`, `MACOS_NOTARY_API_ISSUER_ID`.

### SignPath.io OSS application (deferred)

Currently NOT in use — see
"[Why Windows Authenticode is deferred](#why-windows-authenticode-is-deferred)".
Steps for when re-enabling:

1. Apply at [signpath.io/foundation](https://signpath.io/foundation).
   Provide the repository URL, MIT license, and a short description.
   Approval is case-by-case and may take 1-4 weeks.
2. Once approved: create the project, define a signing policy, and
   add a `GitHub Actions` Trusted Build System to that policy bound
   to `sho7650/sample_account_rust` /
   `.github/workflows/release.yml` / `refs/tags/v*`.
3. User profile → API tokens → create a token scoped to that policy.

### GitHub environment

1. Repository → Settings → Environments → New environment named
   `release-signing`.
2. Add all 10 secrets above.
3. (Recommended) Enable "Required reviewers" with the maintainer's
   account. Every release will then pause for explicit approval before
   signing begins. Secrets cannot be exfiltrated by an unreviewed PR
   change because the environment is only attached to jobs running on
   `main`-merged tags.

## Verifying the pipeline before merging changes to it

The `release.yml` workflow has a `workflow_dispatch` entry point with
a `tag` input. Use a throwaway tag to dry-run the full sign/notarize/
staple/cosign chain without burning a real release.

```sh
# Create a throwaway tag
git tag v0.0.0-signing-test
git push origin v0.0.0-signing-test

# Create a draft GitHub Release for that tag (the workflow uploads to
# an existing release; it does not create one).
gh release create v0.0.0-signing-test \
  --draft \
  --title "Signing dry-run, do not use" \
  --notes "Throwaway release for testing release.yml changes."

# Trigger the upload-assets job manually.
gh workflow run release.yml -f tag=v0.0.0-signing-test

# Watch it.
gh run watch
```

After the run finishes, verify each artifact on a clean machine:

- **Linux**: `cosign verify-blob ...` should print `Verified OK`.
- **macOS**: download the `.tar.gz` via Safari (sets the quarantine
  xattr), extract, run `./sample_account --help`. No Gatekeeper popup
  should appear and no `xattr -d` should be needed. Then disconnect
  from the network and run again — staple is offline-verifiable.
- **Windows**: download the `.zip` via Edge (sets MotW), extract, run
  `sample_account.exe --help`. SmartScreen warning IS expected (the
  exe is not Authenticode-signed yet). Click "More info" → "Run
  anyway" to proceed. The `.cosign.bundle` should still verify.

Once verified, delete the throwaway:

```sh
gh release delete v0.0.0-signing-test --yes --cleanup-tag
```

## Rotation

### Apple Developer ID certificate (5-year cycle)

The cert in your Apple Developer account expires every 5 years. Action:

1. Generate a new CSR + new Developer ID Application certificate.
2. Re-export `.p12`, re-base64, update `MACOS_DEVELOPER_ID_P12_BASE64`
   and `MACOS_DEVELOPER_ID_P12_PASSWORD` and
   `MACOS_DEVELOPER_ID_IDENTITY` in the GitHub environment.
3. **Old binaries continue to work** because each signing run captures
   a `--timestamp` from Apple's TSA. An expired signing cert does not
   retroactively invalidate already-notarized binaries — the
   notarization record in Apple's database is matched by the
   binary's signature hash, not by cert validity.

### App Store Connect API key

There is no enforced rotation, but treat as you would any API
credential — rotate at least annually. Generate a new key, update
secrets, revoke the old key.

### SignPath API token (when re-enabled)

Recommended annual rotation. Generate a new token in SignPath, update
`SIGNPATH_API_TOKEN`, revoke the old one. The signing certificate
itself is owned by SignPath and rotated by them.

### Apple Developer Program annual renewal

$99/yr. Set a calendar reminder. **If the membership lapses**, no new
binaries can be notarized. Existing notarized binaries keep working
as long as Gatekeeper can reach Apple's servers at first launch on
each user's Mac (the notarization record stays in Apple's database
even after membership lapse, until Apple actively revokes it).

## Failure modes & recovery

| Symptom | Likely cause | Recovery |
|---|---|---|
| `notarytool submit` returns "Invalid" | Hardened Runtime missing, or the binary uses a disallowed entitlement. | Re-run with `--options=runtime` (already set). The notarytool log URL is printed on failure — open it for the per-issue list. Our binary uses no JIT / no special frameworks, so this should not happen for the standard build. |
| `notarytool submit --wait` times out at 30 min | Apple notary service slow / incident. | Re-run via `workflow_dispatch` with the same tag once Apple is back. The workflow uses `gh release upload --clobber` so re-runs are idempotent. |
| Staple exits 73 / "CloudKit returned error 4096" | The binary is a **standalone Mach-O**, which `xcrun stapler` does not support — Apple only staples `.app` / `.dmg` / `.pkg` / `.kext` / `.dext`. The error is misleading; it is not actually CloudKit propagation. | The workflow does not call stapler at all — see "Why macOS notarization is NOT stapled" above for the rationale and the upgrade path to `.pkg` distribution if offline-verifiable signing is required. |
| `codesign` complains "no identity found" | `.p12` import failed silently OR the `MACOS_DEVELOPER_ID_IDENTITY` string does not match what was imported. | Verify the `.p12` opens locally with the stored password; verify the IDENTITY string with `security find-identity -v -p codesigning` on a Mac that has the cert imported. |
| SignPath action returns 403 (when re-enabled) | API token revoked/expired, or Trusted Build System not configured on the policy. | Issue a new token; add a `GitHub Actions` Trusted Build System to the policy. |
| SignPath action returns 404 (when re-enabled) | `SIGNPATH_PROJECT_SLUG` or `SIGNPATH_SIGNING_POLICY_SLUG` mismatch. | Verify slugs in the SignPath dashboard (URL-friendly form, not display name). |
| `cosign sign-blob` fails with OIDC error | Job missing `id-token: write` permission, or workflow run was triggered from a fork (forks have no OIDC). | Workflow already declares the permission; this should only happen if the job is restructured. Forks are filtered out by `if: github.event_name == 'push'` etc. |
| `gh release upload` returns "release not found" | The `tag` input does not match an existing release on the repository. | Ensure the release exists (release-please creates it on the push:main path; `workflow_dispatch` requires creating it manually first). |

## Cost ledger

| Item | Recurring | Ad-hoc | Notes |
|---|---|---|---|
| Apple Developer Program | $99 / yr | — | Required for macOS notarization. |
| SignPath.io Foundation OSS | $0 | — | Free tier subject to ongoing OSS eligibility. |
| Sigstore / cosign keyless | $0 | — | Public Sigstore infrastructure; no account needed. |
| GitHub Actions minutes | within free tier | — | Single-target CI runs are cheap; release runs ~10-15 min total. |
| Apple Developer ID cert | (included) | (included) | 5-year validity inside Apple Developer membership. |
| App Store Connect API key | (included) | — | Rotate annually for hygiene; included in membership. |

If SignPath.io OSS approval is denied, the next-cheapest alternative
is **Azure Trusted Signing** at ~$10/mo (~$120/yr). Document that in
this file if/when that fallback is invoked.

## Out of scope (future)

- Intel macOS (`x86_64-apple-darwin`) — same flow, different matrix entry.
- Linux ARM (`aarch64-unknown-linux-gnu`) — would also benefit from cosign.
- `.deb` / `.rpm` / AppImage packaging.
- Mac App Store / Microsoft Store distribution.
- Homebrew tap, winget manifest.
- SLSA Level 3 build provenance via `actions/attest-build-provenance`.
- Reproducible builds.
