# Security policy

## Supported versions

The latest minor release of the 3.x line gets fixes. Fixes ship as a patch release of the
Rust crate and the Python package at once (they share one version number).

| version | supported |
|---|---|
| 3.9.x | yes |
| < 3.9 | no; upgrade to the latest 3.x (no breaking changes within 3.x) |

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's private
vulnerability reporting on this repository: the Security tab, then "Report a
vulnerability" (https://github.com/Mattbusel/fin-primitives/security/advisories/new).
The report stays private between you and the maintainer until an advisory is published.

What happens next:

- You get a first reply within 14 days, usually much sooner.
- If the problem is confirmed, the fix ships as a patch release of the crate and the Python
  package, and a GitHub security advisory (with a CVE requested through GitHub when the
  issue warrants one) is published at the same time. The advisory credits you unless you
  ask not to be named.
- The target is a released fix within 60 days of the report. If that is not possible, the
  advisory is coordinated with you before anything is made public.

A wrong number from an indicator is a bug, not a security problem: open a normal issue for
that. A panic, hang or unbounded memory use reachable from untrusted input is in scope.

## Scope

fin-primitives is a computation library: it places no orders, opens no network
connections and reads no files. The Rust crate is `#![forbid(unsafe_code)]` (the PyO3
bindings behind the `python` feature are the only place that crosses into Python). The
Python wheels are built and published from GitHub Actions with PyPI trusted publishing and
carry provenance attestations.
