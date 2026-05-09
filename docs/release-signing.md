# Release signing — maintainer runbook

This document is the operational reference for the signing chain in
`.github/workflows/release.yml`. End-user *verification* commands live
in the README's "Verifying release artifacts" section.

## What gets signed

| Target | What | Tool | Cost |
|---|---|---|---|
| **Linux x86_64** | The `.tar.gz` archive | cosign keyless (sigstore OIDC) | $0 |
| **macOS aarch64** | The Mach-O binary, then the `.tar.gz` archive | Apple Developer ID Application + Hardened Runtime + `xcrun notarytool` + `xcrun stapler`, then cosign keyless | Apple Developer Program: **$99 / yr** |
| **Windows x86_64** | The `.exe`, then the `.zip` archive | SignPath.io Authenticode signing, then cosign keyless | SignPath Foundation OSS: **$0** (subject to OSS approval) |

Total recurring cash cost: **$99 / yr** (Apple). Apple cert is valid
5 years; the Apple Developer Program membership renews annually.

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

All 10 secrets MUST live in a single GitHub environment named
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
| `SIGNPATH_API_TOKEN` | SignPath.io → User profile → API tokens → New token, scoped to a single signing policy. | Rotate annually. |
| `SIGNPATH_ORGANIZATION_ID` | SignPath.io → organization settings → ID. | UUID. Not strictly secret but kept here for symmetry. |
| `SIGNPATH_PROJECT_SLUG` | SignPath.io → project settings → slug. | E.g. `sample-account`. |
| `SIGNPATH_SIGNING_POLICY_SLUG` | SignPath.io → project → signing policies → slug of the release policy. | E.g. `release-signing`. |

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

### SignPath.io OSS application

1. Apply at [signpath.io/foundation](https://signpath.io/foundation).
   Provide the repository URL, MIT license, and a short description.
   Approval is case-by-case and may take 1-4 weeks.
2. Once approved: create the project, add the GitHub repository as a
   trusted source (OIDC binding to `sho7650/sample_account_rust`),
   define a signing policy (e.g. `release-signing`) that:
   - allows signing only from tagged workflow runs,
   - binds to the `release` workflow file,
   - optionally requires human approval per signing request.
3. User profile → API tokens → create a token scoped to that policy.
   This is `SIGNPATH_API_TOKEN`.

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
  `sample_account.exe --help`. SmartScreen warning should be reduced
  or absent. `signtool verify /pa /v sample_account.exe` should pass.

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
   a `--timestamp` from Apple's TSA, and the staple is offline. An
   expired signing cert does not retroactively invalidate already-
   notarized binaries.

### App Store Connect API key

There is no enforced rotation, but treat as you would any API
credential — rotate at least annually. Generate a new key, update
secrets, revoke the old key.

### SignPath API token

Recommended annual rotation. Generate a new token in SignPath, update
`SIGNPATH_API_TOKEN`, revoke the old one. The signing certificate
itself is owned by SignPath and rotated by them.

### Apple Developer Program annual renewal

$99/yr. Set a calendar reminder. **If the membership lapses**, no new
binaries can be notarized; existing stapled releases continue to work
because the staple is offline-verifiable.

## Failure modes & recovery

| Symptom | Likely cause | Recovery |
|---|---|---|
| `notarytool submit` returns "Invalid" | Hardened Runtime missing, or the binary uses a disallowed entitlement. | Re-run with `--options=runtime` (already set). The notarytool log URL is printed on failure — open it for the per-issue list. Our binary uses no JIT / no special frameworks, so this should not happen for the standard build. |
| `notarytool submit --wait` times out at 30 min | Apple notary service slow / incident. | Re-run via `workflow_dispatch` with the same tag once Apple is back. The workflow uses `gh release upload --clobber` so re-runs are idempotent. |
| Staple shows `CloudKit returned error 4096` | Notarization happened but the ticket has not propagated yet. | Wait 5-10 min and re-run the staple step (re-trigger the workflow). |
| `codesign` complains "no identity found" | `.p12` import failed silently OR the `MACOS_DEVELOPER_ID_IDENTITY` string does not match what was imported. | Verify the `.p12` opens locally with the stored password; verify the IDENTITY string with `security find-identity -v -p codesigning` on a Mac that has the cert imported. |
| SignPath action returns 403 | API token revoked, expired, or scoped to a different signing policy. | Issue a new token in SignPath, scoped to the release-signing policy. Update `SIGNPATH_API_TOKEN`. |
| SignPath action returns 404 | `SIGNPATH_PROJECT_SLUG` or `SIGNPATH_SIGNING_POLICY_SLUG` mismatch. | Verify slugs in the SignPath dashboard (URL-friendly form, not display name). |
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
