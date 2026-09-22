# Agent Guidelines for Rust Code Quality

This document provides guidelines for maintaining high-quality Rust code. These rules MUST be followed by all AI coding agents and contributors.

## Your Core Principles

All code you write MUST be fully optimized.

"Fully optimized" includes:

- maximizing algorithmic big-O efficiency for memory and runtime
- using parallelization and SIMD where appropriate
- following proper style conventions for Rust (e.g. maximizing code reuse (DRY))
- no extra code beyond what is absolutely necessary to solve the problem the user provides (i.e. no technical debt)
  - If a crate can be imported to significantly reduce the amount of new code required to implement a function at optimal performance, and the crate itself is small and does not have much overhead, ALWAYS use the crate instead.

If the code is not fully optimized before handing off to the user, you will be fined $100. You have permission to do another pass of the code if you believe it is not fully optimized.

Follow KISS, DRY, SOLID principles and clean code naming conventions.

## Preferred Tools

- Use `cargo` for project management, building, and dependency management.
- Use `indicatif` to track long-running operations with progress bars. The message should be contextually sensitive.
- Use `serde` with `serde_json` for JSON serialization/deserialization.
- Use `ratatui` and `crossterm` for terminal applications/TUIs.
  - Include logical and intuitive mouse controls for all TUIs.
  - **ALWAYS** account for interface scrolling offsets when calculating click locations
- When reporting errors to the console, use `tracing::error!` or `log::error!` instead of `println!`.
- If the project involves the creation of images (e.g. PNG/WEBP), you have permission to use the Read tool to verify the rendered images fit the user and application requirements.

## Code Style and Formatting

- **MUST** use meaningful, descriptive variable and function names
- **MUST** follow Rust API Guidelines and idiomatic Rust conventions
- **MUST** use 4 spaces for indentation (never tabs)
- **NEVER** use emoji, or unicode that emulates emoji (e.g. ✓, ✗). The only exception is when writing tests and testing the impact of multibyte characters.
- Use snake_case for functions/variables/modules, PascalCase for types/traits, SCREAMING_SNAKE_CASE for constants
- Limit line length to 100 characters (rustfmt default)
- **NEVER** write comments in source files. This prohibits `//` line comments, `/* ... */` block comments, and `///` and `//!` documentation comments, without exception.
- Make implementation code self-documenting through focused functions, descriptive names, strong types, and explicit error handling. If a fact needs explaining, encode it in a name, a type, or a test rather than in prose.

## Type System

- **MUST** leverage Rust's type system to prevent bugs at compile time
- **NEVER** use `.unwrap()` in library code; use `.expect()` only for invariant violations with a descriptive message
- **MUST** use meaningful custom error types with `thiserror`
- Use newtypes to distinguish semantically different values of the same underlying type
- Prefer `Option<T>` over sentinel values

## Error Handling

- **NEVER** use `.unwrap()` in production code paths
- **MUST** use `Result<T, E>` for fallible operations
- **MUST** use `thiserror` for defining error types and `anyhow` for application-level errors
- **MUST** propagate errors with `?` operator where appropriate
- Provide meaningful error messages with context using `.context()` from `anyhow`

## Function Design

- **MUST** keep functions focused on a single responsibility
- **MUST** prefer borrowing (`&T`, `&mut T`) over ownership when possible
- Limit function parameters to 5 or fewer; use a config struct for more
- Return early to reduce nesting
- Use iterators and combinators over explicit loops where clearer

## Struct and Enum Design

- **MUST** keep types focused on a single responsibility
- **MUST** derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate
- Use `#[derive(Default)]` when a sensible default exists
- Prefer composition over inheritance-like patterns
- Use builder pattern for complex struct construction
- Make fields private by default; provide accessor methods when needed

## Testing

- **MUST** write unit tests for all new functions and types
- **MUST** mock external dependencies (APIs, databases, file systems)
- **MUST** use the built-in `#[test]` attribute and `cargo test`
- Follow the Arrange-Act-Assert pattern
- Do not commit commented-out tests
- Use `#[cfg(test)]` modules for test code

## Imports and Dependencies

- **MUST** avoid wildcard imports (`use module::*`) except for preludes, test modules (`use super::*`), and prelude re-exports
- **MUST** document dependencies in `Cargo.toml` with version constraints
- Use `cargo` for dependency management
- Organize imports: standard library, external crates, local modules
- Use `rustfmt` to automate import formatting

## Rust Best Practices

- **NEVER** use `unsafe`; redesign the implementation around safe Rust instead
- **MUST** call `.clone()` explicitly on non-`Copy` types; avoid hidden clones in closures and iterators
- **MUST** use pattern matching exhaustively; avoid catch-all `_` patterns when possible
- **MUST** use `format!` macro for string formatting
- Use iterators and iterator adapters over manual loops
- Use `enumerate()` instead of manual counter variables
- Prefer `if let` and `while let` for single-pattern matching

## Memory and Performance

- **MUST** avoid unnecessary allocations; prefer `&str` over `String` when possible
- **MUST** use `Cow<'_, str>` when ownership is conditionally needed
- Use `Vec::with_capacity()` when the size is known
- Prefer stack allocation over heap when appropriate
- Use `Arc` and `Rc` judiciously; prefer borrowing

## Benchmarking and Optimization

- **NEVER** run benchmarks in parallel, as the benchmarks will compete for resources and the results will be invalid
- **NEVER** game the benchmarks. Do not manipulate the benchmarks themselves to satisfy any required performance constraints
- **NEVER** run benchmarks with `target-cpu=native` or any other `RUSTFLAGS`
- If benchmarking against another crate or library, ensure the benchmarks are apples-to-apples comparisons
- Ensure benchmark tests are independent. If the tests are dependent due to a feature (e.g. caching), ensure the feature is disabled

## Concurrency

- **MUST** use `Send` and `Sync` bounds appropriately
- **MUST** prefer `tokio` for async runtime in async applications
- **MUST** use `rayon` for CPU-bound parallelism
- Avoid `Mutex` when `RwLock` or lock-free alternatives are appropriate
- Use channels (`mpsc`, `crossbeam`) for message passing

## Security

- **NEVER** store secrets, API keys, or passwords in code. Only store them in `.env`
  - Ensure `.env` is declared in `.gitignore`
- **MUST** use environment variables for sensitive configuration via `dotenvy` or `std::env`
- **NEVER** log sensitive information (passwords, tokens, PII)
- Use `secrecy` crate for sensitive data types

## Version Control

- **MUST** write clear, descriptive commit messages
- **NEVER** commit commented-out code; delete it
- **NEVER** commit debug `println!` statements or `dbg!` macros
- **NEVER** commit credentials or sensitive data

## Tools

- **MUST** use `rustfmt` for code formatting
- **MUST** use `clippy` for linting and follow its suggestions
- **MUST** ensure code compiles with no warnings (use `-D warnings` flag in CI, not `#![deny(warnings)]` in source)
- Use `cargo` for building, testing, and dependency management
- Use `cargo test` for running tests
- **NEVER** uses the `Explore` tool for `Cargo.lock`: it is large and irrelevant. Read `Cargo.lock` **ONLY** if it's extremely relevant.

## Before Committing

- [ ] All tests pass (`cargo test`)
- [ ] No compiler warnings (`cargo build`)
- [ ] Clippy passes (`cargo clippy -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --check`)
- [ ] No comments of any kind, including doc comments; internal APIs use the narrowest practical visibility
- [ ] No commented-out code or debug statements
- [ ] No hardcoded credentials

---

**Remember:** Prioritize clarity and maintainability over cleverness. This is your core directive.
