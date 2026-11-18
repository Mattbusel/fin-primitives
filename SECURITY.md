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
vulnerability". A confirmed problem gets a patch release and a GitHub security advisory.

## Scope

fin-primitives is a computation library: it places no orders, opens no network
connections and reads no files. The Rust crate is `#![forbid(unsafe_code)]` (the PyO3
bindings behind the `python` feature are the only place that crosses into Python). The
Python wheels are built and published from GitHub Actions with PyPI trusted publishing and
carry provenance attestations.
