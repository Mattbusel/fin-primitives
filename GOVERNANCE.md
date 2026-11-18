# Governance

## Who decides

fin-primitives has one maintainer: Matthew Busel ([@Mattbusel](https://github.com/Mattbusel)),
who works on it through Tensorust LLC. The maintainer reviews and merges pull requests,
decides what goes into a release, and publishes releases. There is no committee and no
voting; decisions are made in the open on issues and pull requests, with the reasoning
written down there.

Anyone can propose a change by opening an issue or a pull request. See
[CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test and submit changes. Every change
lands on `main` through a pull request or a reviewed commit, and CI (formatting, clippy with
warnings as errors, the full test suite, the MSRV build and the Python wheel tests) must be
green before it is merged.

## Releases

The Rust crate and the Python package share one version number and follow
[Semantic Versioning](https://semver.org/). Within the 3.x line there are no breaking changes.

1. Bump the version in `Cargo.toml` and `python/pyproject.toml`, and add a dated entry to
   [CHANGELOG.md](CHANGELOG.md) that lists every user-visible change and any fixed
   vulnerability.
2. Merge to `main` with CI green.
3. Tag the commit `vX.Y.Z` (crate) and `py-vX.Y.Z` (Python), and publish a GitHub release
   whose notes come from the changelog entry.
4. The crate is published to [crates.io](https://crates.io/crates/fin-primitives) from the
   tagged commit with `cargo publish`.
5. The `py-vX.Y.Z` tag runs [python-wheels.yml](.github/workflows/python-wheels.yml), which
   builds and tests every wheel and the sdist, then uploads them to
   [PyPI](https://pypi.org/project/fin-primitives/) with trusted publishing (OIDC, no stored
   token) and PEP 740 provenance attestations. The upload runs in the dedicated `pypi`
   environment and refuses files whose version does not match the tag.

Security fixes follow [SECURITY.md](SECURITY.md) and ship as a patch release.

## Continuity

The project has a single maintainer, so the plan for the maintainer becoming unavailable is
written down here:

- Everything needed to build, test and release is in this repository: the code, the CI
  workflows and the release steps above. PyPI releases come from a workflow, not from a
  personal machine or a stored token.
- The code is MIT licensed, so anyone can fork and continue it without asking.
- When a regular contributor emerges, the maintainer will offer them commit access and
  co-owner rights on crates.io and PyPI, so that releases do not depend on one person.
- If the maintainer decides to step back, the repository and the crates.io and PyPI
  projects will be handed to a willing maintainer and the change announced in the README
  and in a pinned issue. If nobody takes it over, the repository will be archived with a
  note saying so, rather than left looking maintained.

## Changes to this document

Changes to governance are made by pull request like any other change.
