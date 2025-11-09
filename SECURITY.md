# Security policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | ✅        |

Pre-1.0 releases follow the rolling-support model: only the most recent
minor line receives security patches.

## Reporting a vulnerability

Please report security issues privately. Do **not** open a public GitHub
issue for sensitive disclosures.

- Email: `security@themankindproject.dev`
- GitHub: use [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
  on the `txtfp` repository.

## Disclosure timeline

- **48 hours**: acknowledgement of the report.
- **7 days**: triage and provisional severity assessment.
- **30 days**: target window for a fix release. Coordinated disclosure
  beyond this window is negotiated with the reporter.
- A CVE will be requested for any vulnerability rated High or Critical
  per CVSS v3.1.

## Scope

In scope:

- Memory safety (panics, unsoundness) in `txtfp`'s own code.
- Hash output divergence from documented byte-layout (semver violation).
- Denial-of-service via crafted inputs that exceed documented bounds.
- Improper handling of API keys in cloud-provider integrations.

Out of scope:

- Vulnerabilities in upstream dependencies — please report those to the
  upstream maintainers; we will pull the fix once it lands.
- Cryptographic-level attacks on the hash families (MurmurHash3, xxHash,
  SimHash). These are non-cryptographic by design.
