# Windows Code Signing Options

Last updated: 2026-07-05

This document compares practical signing choices for `vibe-usage-app-windows`, a Tauri Windows app that currently publishes an NSIS `.exe` installer through GitHub Releases.

The goal is to help the client choose a route that balances:

- Money cost
- Human / operations cost
- Public Windows trust
- CI automation
- Open-source project constraints

## Executive Summary

| Option | Money cost | Human cost | CI fit | Public user experience | Recommended when |
|---|---:|---:|---|---|---|
| SignPath Foundation for open source | Free if accepted | Medium | Good | Signed installer for public releases, but publisher is tied to SignPath Foundation trust flow | Best free route for public open-source releases |
| Microsoft Trusted Signing / Azure Artifact Signing | Paid monthly | Medium | Excellent | Public-trusted signatures, no hardware token | Best paid route if client wants company-backed signing in CI |
| Traditional OV / IV code signing certificate | Paid yearly plus token/cloud signing | Medium to high | Medium | Public-trusted signatures, SmartScreen reputation builds over time | Good if client wants standard CA certificate outside Azure |
| Traditional EV code signing certificate | Higher paid yearly plus token/cloud signing | High | Medium | Public-trusted signatures, stronger identity signal | Usually not necessary for this app unless procurement requires EV |
| Microsoft Store distribution | Store signing can be free for package signing | High product/distribution work | Medium | Best consumer trust inside Store channel | Good as an additional channel, not a replacement for GitHub `.exe` releases |
| Self-signed local certificate | Free | Low | Poor for public use | Works only on machines that manually trust the cert | QA/internal testing only |
| Enterprise/private trust | Usually no public CA cost | Medium to high | Good inside enterprise | Works only for devices managed by that organization | Internal company deployment only |
| Unsigned release plus SHA256/GitHub attestation | Free | Low | Excellent | Does not solve Unknown Publisher or Smart App Control | Not acceptable as the only public release path |

For this project, the practical recommendation is:

1. If the client requires zero money cost and accepts open-source conditions: apply for SignPath Foundation.
2. If the client can pay a small recurring fee and wants a smoother CI-owned workflow: use Microsoft Trusted Signing.
3. If the client wants a traditional certificate under their legal company name and does not want Azure: buy OV code signing with cloud signing or HSM support.
4. Keep the current self-signed certificate only for local QA. It is not a public distribution solution.

## Important Concepts

### Authenticode Is The Windows Runtime Trust Layer

For this app, the installer and executable need Microsoft Authenticode signatures:

- `target/release/vibe-usage-app.exe`
- `target/release/bundle/nsis/*-setup.exe`
- Any other first-party `.exe` / `.dll` that the app launches or ships

Signing the Git tag, adding a checksum, or publishing through GitHub Releases is useful, but it does not replace Authenticode.

### Signing Does Not Instantly Remove Every SmartScreen Warning

A valid signature normally removes `Unknown Publisher` and is required for Smart App Control compatibility. However, Microsoft Defender SmartScreen also uses reputation. A new app, new certificate, or new publisher can still show a reputation warning until enough trusted installs accumulate.

EV certificates and Microsoft-managed signing may help establish identity more strongly, but modern Windows app reputation is still not a pure "buy certificate and all warnings disappear forever" system.

### Timestamping Is Required

Every production signature should include a trusted timestamp. Without timestamping, signatures may become invalid or less useful after a certificate expires.

Our current script defaults to:

```text
http://timestamp.digicert.com
```

## Option 1: SignPath Foundation For Open Source

### What It Is

SignPath Foundation provides free code signing for qualifying open-source projects. The project applies, SignPath reviews it, and approved release artifacts are signed through SignPath's controlled process.

Official links:

- [SignPath Foundation](https://signpath.org/)
- [Terms and conditions](https://signpath.org/terms)
- [Apply for free code signing](https://signpath.org/apply)
- [Listed open-source projects](https://signpath.org/projects)

### Money Cost

Expected direct cost: `0`.

No commercial certificate purchase is required if the project is accepted.

### Human Cost

Estimated setup effort: `1-3 person-days`, plus external review time.

Typical work:

- Ensure the repository has an OSI-compatible open-source license.
- Make the source, build instructions, release artifacts, and changelog clear.
- Add or improve security / code signing policy documentation.
- Make the CI release process reproducible.
- Configure SignPath project, signing policy, artifact upload/download, and release approval.
- Keep project members using MFA and follow SignPath's release approval rules.

External review time is not fully controllable. Plan for days to weeks, not minutes.

### CI Fit

Good, but it is a separate integration from PFX/thumbprint signing.

Likely CI shape:

1. GitHub Actions builds unsigned artifacts.
2. CI uploads the installer and app binaries to SignPath.
3. SignPath applies the policy and signs the artifacts.
4. A maintainer approves the signing request if policy requires it.
5. CI or a release maintainer downloads the signed artifacts and publishes them.

### User Experience

Good for a free open-source project:

- Installer is Authenticode-signed.
- Public users no longer see a completely unsigned publisher experience.
- Smart App Control should have a valid-signature path.

Caveat:

- The displayed signing identity may not be the client company's own legal publisher name. This is a tradeoff of using a foundation signing program instead of buying a company certificate.

### Risks

- Project may not qualify.
- Review may take time.
- Signing must follow SignPath's policies.
- If the client later closes source or bundles proprietary code, this route may stop being appropriate.

### Fit For Vibe Usage

Best free public-release route if the project remains open source and the client accepts the foundation-based signing identity.

## Option 2: Microsoft Trusted Signing / Azure Artifact Signing

### What It Is

Microsoft Trusted Signing is Microsoft's cloud signing service for Windows artifacts. It avoids local private-key handling and is designed for CI/CD signing.

Official links:

- [Trusted Signing overview](https://learn.microsoft.com/en-us/azure/trusted-signing/overview)
- [Quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)
- [Signing integrations](https://learn.microsoft.com/en-us/azure/artifact-signing/how-to-signing-integrations)
- [Azure pricing page](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/)
- [Microsoft MSIX signing options overview](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview)

### Money Cost

Paid recurring service.

Microsoft's MSIX signing documentation describes Trusted Signing as a low-cost monthly option and gives a Basic-tier estimate around `$10/month`; the Azure pricing page should be checked at purchase time for the exact region, tier, quota, and currency.

### Human Cost

Estimated setup effort: `1-3 person-days`, plus identity validation time.

Typical work:

- Client creates or uses an Azure subscription.
- Client completes Microsoft identity validation.
- Create Trusted Signing account and certificate profile.
- Add GitHub Actions secrets or federated credentials.
- Update release workflow to call Microsoft signing tools.
- Sign both the app executable and the NSIS installer.

Identity validation can take real calendar time. Microsoft documentation notes country/region and identity restrictions, so the client must confirm eligibility before choosing this route.

### CI Fit

Excellent.

This is the cleanest paid CI option because:

- No private key needs to live in GitHub secrets.
- No USB hardware token is needed on a GitHub-hosted runner.
- Signing can be part of the normal release workflow.

### User Experience

Good:

- Public-trusted Authenticode signature.
- Removes `Unknown Publisher`.
- Better path through Smart App Control than unsigned builds.

SmartScreen reputation may still need time for a new app or publisher.

### Risks

- Paid service.
- Requires Microsoft identity validation.
- Availability can depend on client country/region and account type.
- Ties signing process to Azure.

### Fit For Vibe Usage

Best paid choice if the client can accept recurring cost and wants reliable CI signing without certificate-token operations.

## Option 3: Traditional IV / OV Code Signing Certificate

### What It Is

The client buys a code signing certificate from a public CA. The CA validates an individual identity (IV) or organization identity (OV). The private key is usually protected by a hardware token, HSM, or cloud signing service.

Example official links:

- [SSL.com code signing certificates](https://www.ssl.com/certificates/code-signing/)
- [SSL.com IV code signing](https://www.ssl.com/products/software-integrity/code-signing/iv/)
- [SSL.com OV code signing](https://www.ssl.com/products/software-integrity/code-signing/ov/)
- [SSL.com eSigner for code](https://www.ssl.com/products/software-integrity/esigner-for-code/)
- [Microsoft MSIX signing options overview](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview)

### Money Cost

Typical annual cost: roughly `$100-$500+/year`, depending on CA, identity type, term, reseller, token, and cloud signing option.

Examples checked on 2026-07-05:

- SSL.com IV code signing showed pricing starting at `$129/year`.
- SSL.com OV code signing showed pricing starting at `$129/year`.
- SSL.com eSigner showed a monthly cloud signing add-on starting at `$15/month`.
- Microsoft's MSIX signing overview describes traditional CA certificates as commonly around `$300-$500/year`.

The client should confirm final quote before purchase because CA pricing changes frequently.

### Human Cost

Estimated setup effort: `1-5 person-days`, plus validation and delivery time.

Typical work:

- Choose CA and certificate type.
- Prepare identity documents.
- Complete phone, email, business registry, or personal identity validation.
- Decide private-key storage:
  - Cloud signing service
  - HSM
  - Physical token
  - Windows certificate store on a controlled build machine
- Configure CI signing.
- Rotate and renew certificate before expiry.

### CI Fit

Medium.

CI is straightforward if the CA provides a cloud signing API or a PFX-like importable certificate. It is harder if signing requires a physical USB token, because GitHub-hosted runners cannot use a local USB token.

The current repository already supports:

- `WINDOWS_CODESIGN_PFX_BASE64`
- `WINDOWS_CODESIGN_PFX_PASSWORD`
- `WINDOWS_CODESIGN_CERT_THUMBPRINT`
- `WINDOWS_CODESIGN_TIMESTAMP_URL`

However, many modern public code signing certificates require protected private keys, so the final implementation may need cloud signing or a self-hosted runner.

### User Experience

Good:

- Removes `Unknown Publisher`.
- Standard public-trusted Authenticode signature.
- Publisher can be the individual or company validated by the CA.

SmartScreen reputation still builds over time.

### Risks

- Certificate renewal and private-key custody become an operational responsibility.
- Hardware-token CI can be fragile.
- A leaked key or compromised CI signing flow is serious.

### Fit For Vibe Usage

Good if the client wants their own publisher name and does not want Azure. Prefer OV under the client's legal company if available. IV under an individual works, but shows the individual's identity rather than the company.

## Option 4: Traditional EV Code Signing Certificate

### What It Is

EV code signing performs stricter organization validation and stores keys in stronger protected hardware/cloud environments. It is still Authenticode signing, just with a higher-assurance identity profile.

Example official links:

- [SSL.com EV code signing](https://www.ssl.com/products/software-integrity/code-signing/ev/)
- [Microsoft MSIX signing options overview](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview)

### Money Cost

Typical annual cost: `$300-$1000+/year`, depending on CA, term, token, and cloud signing requirements.

Example checked on 2026-07-05:

- SSL.com EV code signing showed pricing starting at `$349/year`.

### Human Cost

Estimated setup effort: `2-7 person-days`, plus validation time.

Typical work:

- More strict organization validation than OV.
- Hardware/cloud key custody.
- CI integration is similar to OV but often stricter.
- Renewal and signing audit process need to be maintained.

### CI Fit

Medium.

Works if the CA offers cloud signing or if the client can maintain a secure self-hosted signing machine. It is not ideal if the only key material is on a physical token that someone must manually operate.

### User Experience

Good, but not magical:

- Stronger identity signal.
- Public-trusted Authenticode signature.
- May help with reputation, but it is not a guaranteed permanent bypass for every SmartScreen case.

### Risks

- Higher cost.
- More paperwork.
- Usually unnecessary for a normal open-source desktop utility.

### Fit For Vibe Usage

Not the first choice unless the client explicitly requires EV, has procurement rules that require EV, or plans to sign driver-like components. For this app, OV or Microsoft Trusted Signing is usually enough.

## Option 5: Microsoft Store Distribution

### What It Is

The app is distributed through Microsoft Store / Partner Center. Store package signing can avoid the need to buy a separate certificate for that Store distribution path.

Official links:

- [Microsoft Store registration](https://developer.microsoft.com/en-us/microsoft-store/register/)
- [Microsoft MSIX signing options overview](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview)
- [Partner Center app publishing overview](https://learn.microsoft.com/en-us/windows/apps/publish/)

### Money Cost

Code-signing certificate cost for Store-signed package path: usually `0`.

There may still be:

- Partner Center account requirements.
- Company onboarding effort.
- Engineering time to package and pass Store requirements.

The client should check the current Partner Center account terms before committing.

### Human Cost

Estimated setup effort: `3-10 person-days`.

Typical work:

- Create Partner Center account.
- Prepare store listing, privacy policy, screenshots, age ratings, support links.
- Ensure app behavior passes certification.
- Potentially produce MSIX or Store-accepted package/installer flow.
- Maintain Store releases separately from GitHub Releases.

### CI Fit

Medium.

It can be automated, but it is a distribution pipeline, not just a signing step.

### User Experience

Best for users who install from Microsoft Store:

- Store channel has high trust.
- Updates can be managed through Store.

But it does not automatically solve the trust problem for GitHub `.exe` installers. If users still download `VibeUsage-x.y.z-Windows-Setup.exe` from GitHub, that `.exe` should still be Authenticode-signed.

### Risks

- Adds a new distribution channel and release process.
- Store certification can block releases.
- Some open-source users prefer direct GitHub downloads.

### Fit For Vibe Usage

Good as an additional channel later. It is not the fastest fix for current GitHub Release installer warnings.

## Option 6: Self-Signed Local Certificate

### What It Is

Generate a local code signing certificate and manually trust it on a machine.

This is what we used for local QA:

```text
CN=Vibe Usage Local QA Signing
```

### Money Cost

`0`.

### Human Cost

Estimated setup effort: `0.5 person-day`.

Typical work:

- Generate certificate.
- Import it into Current User or Local Machine Trusted Root.
- Import it into Trusted Publishers.
- Sign local `.exe` files.
- Remove certificate after QA if no longer needed.

### CI Fit

Poor for public use.

Technically CI can sign with a self-signed certificate, but users would still need to import and trust that certificate manually. That is not acceptable for normal public distribution.

### User Experience

Only good on machines where the certificate is explicitly trusted.

It does not help random users downloading from GitHub.

### Risks

- Training users to trust random certificates is dangerous.
- Smart App Control and enterprise policies may still block untrusted environments.
- If the private key leaks, anyone can create binaries that appear trusted on machines where the root was imported.

### Fit For Vibe Usage

QA only. Keep it for local validation, not release distribution.

## Option 7: Enterprise / Private Trust

### What It Is

A company uses its own internal CA, Intune, Group Policy, or device management system to distribute trust to managed Windows devices.

### Money Cost

Direct public certificate cost can be `0`, but this assumes the organization already has device management infrastructure.

Real cost is IT operations time.

### Human Cost

Estimated setup effort: `2-10 person-days`, depending on the organization.

Typical work:

- Create internal code signing cert.
- Protect private key.
- Deploy trusted root and publisher certs.
- Push installer through Intune/SCCM/GPO or internal portal.
- Maintain revocation and renewal.

### CI Fit

Good inside an enterprise if the client has internal signing infrastructure.

### User Experience

Good only for managed company devices.

Public users outside the organization will not trust the private certificate.

### Fit For Vibe Usage

Only useful if the app is being deployed internally to the client's employees. Not useful for public open-source distribution.

## Option 8: Certum Open Source Code Signing

### What It Is

Certum has offered a lower-cost "Open Source Code Signing" certificate product for open-source developers.

Official link:

- [Certum Open Source Code Signing](https://shop.certum.eu/open-source-code-signing.html)

### Money Cost

Example checked on 2026-07-05:

- Product page showed `EUR 69`.
- Page also showed the product as unavailable at that time.

### Human Cost

Estimated setup effort: `1-5 person-days`, plus availability and validation time.

Typical work:

- Confirm product availability.
- Confirm project eligibility.
- Complete identity validation.
- Handle token or signing tooling.
- Integrate with CI.

### CI Fit

Medium to poor unless a cloud signing flow is available.

If the product depends on a physical card/token, GitHub-hosted CI is awkward and may require manual signing or a self-hosted runner.

### User Experience

Potentially good if available and publicly trusted, but this route should be re-checked before planning around it because availability and terms have changed over time.

### Fit For Vibe Usage

Possible low-cost fallback, but not as clean as SignPath Foundation for free OSS or Microsoft Trusted Signing for CI.

## Option 9: GitHub Attestations, GPG, Checksums, Winget

### What It Is

These are integrity and distribution aids, not replacements for Windows Authenticode signing.

Examples:

- GitHub artifact attestations
- Git tag signing
- SHA256 checksums
- Winget package manifest hashes
- Reproducible build notes

### Money Cost

Usually `0`.

### Human Cost

Estimated setup effort: `0.5-2 person-days`.

### CI Fit

Excellent.

### User Experience

Helpful for advanced users, but it does not remove:

- `Unknown Publisher`
- SmartScreen reputation warnings
- Smart App Control blocks caused by unsigned binaries

### Fit For Vibe Usage

Use as a supplement. Do not treat it as the signing solution.

## Official Documentation Links

Use these links as the client's source-of-truth checklist. Prices, eligibility, region support, and validation requirements can change; confirm them on the official pages before purchase or submission.

### Microsoft Windows Trust And Signing

- Microsoft Trusted Signing overview: https://learn.microsoft.com/en-us/azure/trusted-signing/overview
- Microsoft Trusted Signing quickstart: https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart
- Microsoft Trusted Signing signing integrations: https://learn.microsoft.com/en-us/azure/artifact-signing/how-to-signing-integrations
- Microsoft Trusted Signing / Artifact Signing pricing: https://azure.microsoft.com/en-us/pricing/details/artifact-signing/
- Microsoft MSIX package signing overview: https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview
- Microsoft SignTool documentation: https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool
- Microsoft Smart App Control overview: https://support.microsoft.com/en-us/windows/smart-app-control-in-windows-11-285ea03d-fa88-4d56-882e-6698afdb7003
- Microsoft Smart App Control block explanation: https://support.microsoft.com/en-us/windows/smart-app-control-has-blocked-part-of-this-app-5ba9e63a-9986-4d28-8472-9e28e528044e

### Microsoft Store / Partner Center

- Microsoft Store registration: https://developer.microsoft.com/en-us/microsoft-store/register/
- Partner Center app publishing overview: https://learn.microsoft.com/en-us/windows/apps/publish/
- Create a developer account: https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/create-a-developer-account
- Publish your app overview: https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/overview
- Package and submit apps: https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/overview

### SignPath Foundation

- SignPath Foundation home: https://signpath.org/
- SignPath Foundation application: https://signpath.org/apply
- SignPath Foundation terms: https://signpath.org/terms
- SignPath Foundation project list: https://signpath.org/projects
- SignPath documentation home: https://about.signpath.io/documentation

### Traditional Certificate Authorities

These are official vendor pages for quote gathering and eligibility checks. They are not endorsements.

- SSL.com code signing overview: https://www.ssl.com/certificates/code-signing/
- SSL.com IV code signing: https://www.ssl.com/products/software-integrity/code-signing/iv/
- SSL.com OV code signing: https://www.ssl.com/products/software-integrity/code-signing/ov/
- SSL.com EV code signing: https://www.ssl.com/products/software-integrity/code-signing/ev/
- SSL.com eSigner cloud signing: https://www.ssl.com/products/software-integrity/esigner-for-code/
- Certum Open Source Code Signing: https://shop.certum.eu/open-source-code-signing.html
- DigiCert code signing certificates: https://www.digicert.com/signing/code-signing-certificates
- Sectigo code signing certificates: https://www.sectigo.com/ssl-certificates-tls/code-signing
- GlobalSign code signing certificates: https://www.globalsign.com/en/code-signing-certificate

### Tauri / App Build Tooling

- Tauri Windows code signing: https://v2.tauri.app/distribute/sign/windows/
- Tauri Windows installer distribution: https://v2.tauri.app/distribute/windows-installer/
- Tauri Windows bundler configuration: https://v2.tauri.app/reference/config/#windowsconfig

### GitHub Release Integrity

- GitHub Releases: https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository
- GitHub artifact attestations: https://docs.github.com/en/actions/security-guides/using-artifact-attestations-to-establish-provenance-for-builds
- GitHub signed commits: https://docs.github.com/en/authentication/managing-commit-signature-verification/signing-commits
- GitHub encrypted secrets for Actions: https://docs.github.com/en/actions/security-guides/using-secrets-in-github-actions
- GitHub OpenID Connect for cloud providers: https://docs.github.com/en/actions/deployment/security-hardening-your-deployments/about-security-hardening-with-openid-connect

### Windows Package Manager / Winget

- Windows Package Manager package manifest: https://learn.microsoft.com/en-us/windows/package-manager/package/manifest
- Windows Package Manager package repository: https://learn.microsoft.com/en-us/windows/package-manager/package/repository
- Submit packages to the winget-pkgs repository: https://learn.microsoft.com/en-us/windows/package-manager/package/repository#submitting-a-package

## Decision Matrix

| Requirement | Best option |
|---|---|
| Must be free and public | SignPath Foundation |
| Must be free and only local QA | Self-signed local certificate |
| Must show client's company name | Microsoft Trusted Signing or OV/EV certificate |
| Must work cleanly in GitHub Actions | Microsoft Trusted Signing or SignPath Foundation |
| Must avoid Azure | SignPath Foundation or traditional OV/IV certificate |
| Must avoid third-party review | Traditional OV/IV certificate |
| Must ship through Microsoft Store | Microsoft Store distribution |
| Must support public GitHub `.exe` download | SignPath, Microsoft Trusted Signing, or traditional CA certificate |
| Must be accepted by enterprise managed devices only | Enterprise/private trust |

## Recommended Path For This Project

### Path A: Free OSS Release Path

Choose SignPath Foundation.

Implementation plan:

1. Add or verify license, privacy policy, security policy, and release documentation.
2. Make CI build deterministic enough for review.
3. Apply to SignPath Foundation.
4. Add a SignPath signing job to GitHub Actions.
5. Sign the app executable and installer.
6. Publish only the signed installer to GitHub Releases.

Expected cost:

- Money: `0`
- Engineering: `1-3 person-days`
- Waiting/review: external, likely days to weeks

Main tradeoff:

- Client may not get their own company name as the signing publisher.

### Path B: Client Company Release Path

Choose Microsoft Trusted Signing if paid cost is acceptable.

Implementation plan:

1. Client creates Azure/Partner identity and completes validation.
2. Create Trusted Signing account and certificate profile.
3. Update GitHub Actions to sign with Microsoft signing tools.
4. Keep timestamping enabled.
5. Publish signed artifacts.

Expected cost:

- Money: paid monthly; check Azure pricing at purchase time
- Engineering: `1-3 person-days`
- Validation: external, can take multiple business days

Main tradeoff:

- Paid service and Azure dependency.

### Path C: Traditional Certificate Path

Choose OV code signing certificate with cloud signing support.

Implementation plan:

1. Client buys OV certificate from a public CA.
2. Prefer cloud signing over physical USB token for CI.
3. Add CA signing credentials to GitHub Actions or self-hosted runner.
4. Use existing repo signing variables where possible.
5. Sign app executable and installer.

Expected cost:

- Money: roughly `$100-$500+/year`, plus token/cloud signing fees
- Engineering: `1-5 person-days`
- Validation: external, can take multiple business days

Main tradeoff:

- More certificate custody and renewal work than Microsoft Trusted Signing.

## Current Repository Readiness

The repository already has a release path that supports classic Authenticode signing inputs:

- `WINDOWS_CODESIGN_PFX_BASE64`
- `WINDOWS_CODESIGN_PFX_PASSWORD`
- `WINDOWS_CODESIGN_CERT_THUMBPRINT`
- `WINDOWS_CODESIGN_TIMESTAMP_URL`

This is enough for:

- PFX-based signing, if a certificate provider allows it.
- Thumbprint-based signing, if the certificate is already available in the Windows certificate store on the runner.

It is not yet enough for:

- SignPath Foundation signing flow.
- Microsoft Trusted Signing cloud flow.
- CA-specific cloud signing APIs such as eSigner.

Those need dedicated CI steps.

## What The Client Must Decide

Ask the client to choose one of these:

1. Free OSS route: "We accept SignPath Foundation conditions and publisher tradeoffs."
2. Company identity route: "We will pay for Microsoft Trusted Signing or OV certificate so Windows shows our legal publisher."
3. Store route: "We want Microsoft Store as the primary distribution channel."
4. Internal-only route: "This is only for managed company devices."

For public GitHub Releases with no money cost, the realistic answer is SignPath Foundation. A self-signed certificate is free, but it is not a public signing solution.
