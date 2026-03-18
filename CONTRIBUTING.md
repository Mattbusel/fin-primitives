# Contributing to fin-primitives

Thank you for your interest in contributing! This document explains how to get
started, run tests, and submit changes.

## Development Environment Setup

1. Install Rust via [rustup](https://rustup.rs/) (stable toolchain, 1.75+).
2. Clone the repository:
   ```sh
   git clone https://github.com/Mattbusel/fin-primitives
   cd fin-primitives
   ```
3. Build the project:
   ```sh
   cargo build
   ```

## Running Tests

```sh
cargo test
```

To also run benchmarks:

```sh
cargo bench
```

All tests must pass before submitting a pull request.

## Coding Standards

- Format code with `cargo fmt` before committing.
- Lint with `cargo clippy -- -D warnings`; resolve all warnings.
- Avoid `unwrap()` and `expect()` in production paths; use proper error
  handling with `?` or explicit matching.
- Keep unsafe blocks to an absolute minimum and document every one.
- Public items (types, functions, traits) must have doc comments (`///`).
- Numerical code should include references to the underlying financial
  formulas or standards where applicable.

## Branch and PR Workflow

1. Fork the repository and create a feature branch from `main`:
   ```sh
   git checkout -b feat/your-feature-name
   ```
2. Make your changes, ensuring `cargo fmt`, `cargo clippy`, and `cargo test`
   all pass locally.
3. Push your branch and open a Pull Request against `main`.
4. Fill in the PR template, linking any related issues.
5. At least one maintainer review is required before merging.
6. Squash or rebase commits to keep history clean.

## Reporting Bugs

Please open an issue on GitHub with:

- A clear, descriptive title.
- Steps to reproduce the problem.
- Expected behavior vs. actual behavior.
- Rust version (`rustc --version`) and operating system.
- Any relevant logs or stack traces.
- For numerical bugs, include the exact inputs and expected vs. actual outputs.

## Commit Message Convention

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add Black-Scholes Greeks calculation
fix: correct day-count fraction for ACT/360
docs: add usage example for bond pricing
```

## License

By contributing you agree that your contributions will be licensed under the
same license as this project.
