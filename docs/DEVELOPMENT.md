<div id="top"></div>

# Development

## Design rules

- Browser primitives stay generic and composable.
- Shared behavior belongs in the Rust engine, not duplicated CLI binaries.
- JavaScript injection is an escape hatch, not the default implementation strategy.
- CLI tools, routines, and future agent runtimes consume the same primitive registry.
- Runtime state and build artifacts stay outside the repository.
- Browser test runs should leave no stray Chromium processes behind.

## Tests

Run the full suite:

```bash
cargo test --all-targets
```

Deterministic browser fixtures live in `tests/fixtures/` for delayed images, DOM rerenders, visibility, and downloads. Browser integration runs should use these local fixtures rather than public sites.

## Generated tool documentation

Regenerate the tool index:

```bash
scripts/build-tool-index.sh
```

Verify it is current:

```bash
scripts/check-tool-index.sh
```

CI performs the same freshness check.

## Runtime cleanup

```bash
scripts/clean-runtime.sh
scripts/clean-runtime.sh --build
```

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
