set positional-arguments

# Note: help messages should be 1 line long as required by just.

# Print a help message.
help:
    just --list

# Run `cargo hack --feature-powerset` on crates
powerset *args:
    NEXTEST_NO_TESTS=pass cargo hack --feature-powerset --workspace "$@"

# Build docs for crates and direct dependencies
rustdoc *args:
    @cargo tree --depth 1 -e normal --prefix none --workspace --all-features \
        | gawk '{ gsub(" v", "@", $0); printf("%s\n", $1); }' \
        | xargs printf -- '-p %s\n' \
        | RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS='--cfg=doc_cfg' xargs cargo doc --no-deps --all-features {{args}}

# Generate README.md files using `cargo-sync-rdme`.
generate-readmes:
    cargo sync-rdme --toolchain nightly-2026-04-30 -p preinstantiate

# Collect test coverage
coverage:
    #!/usr/bin/env bash
    echo "Running coverage (requires nightly)" >&2
    cargo llvm-cov nextest --all-features --all-targets --lcov --output-path lcov.info
    cargo llvm-cov test --all-features --doc --lcov --output-path lcov-doctest.info
    echo "Wrote output to lcov.info and lcov-doctest.info" >&2
