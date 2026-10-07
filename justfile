# hail — command runner

set shell := ["bash", "-euo", "pipefail", "-c"]

[private]
default:
    @just --list

# All checks: format, lint, unit and integration tests, release-script tests, tmux scenarios, speed
[group('check')]
check: fmt-check lint test-rust test-release test bench

# cargo fmt --check
[group('check')]
fmt-check:
    cargo fmt --check

# clippy with warnings as errors; bash -n and shellcheck over the harness and scripts
[group('check')]
lint:
    cargo clippy --all-targets --quiet -- -D warnings
    bash -n test/run.sh scripts/test-home-manager-module.sh scripts/bench.sh
    shellcheck -S warning test/run.sh scripts/test-home-manager-module.sh scripts/bench.sh

# Unit and integration tests (no tmux needed)
[group('check')]
test-rust:
    cargo test --quiet

# Scenario harness on a scratch tmux server (-L hailtest); never touches yours.
# Scenario 23 holds an empty deliver to HAIL_TEST_DELIVER_MS (default 25 ms,
# an idle-machine budget); CI sets 100 for hosted runners, and so should a
# loaded dev machine.
[group('check')]
test:
    cargo build --quiet
    bash test/run.sh

# Hot paths against their budgets (spec §9); fails at 3x on a loaded host
[group('check')]
bench:
    bash scripts/bench.sh

# Unit tests for scripts/release.py
[group('check')]
test-release:
    python3 scripts/test_release.py

# Evaluate the Home Manager module against stub options
[group('check')]
test-home-manager-module:
    bash scripts/test-home-manager-module.sh

# Publish the described jj change: run `just check` on exactly it, then move main to it and push
[group('vcs')]
land *args:
    scripts/jj-land.sh {{args}}

# Build the Nix package and print its version
[group('build')]
build:
    out="$(nix build --no-link --print-out-paths .)" && "$out/bin/hail" --version

# Set the version in Cargo.toml and scaffold the CHANGELOG entry
[group('release')]
[arg('version', pattern='[0-9]+\.[0-9]+\.[0-9]+', help='Semver release, e.g. 0.3.1')]
release-bump version:
    python3 scripts/release.py bump {{quote(version)}}

# Every advertised Nix package output is in the public Cachix cache
[group('release')]
cache-verify:
    python3 scripts/release.py cache-verify

# Release readiness: versions agree, changelog filled, clean tree, checks, Nix build
[group('release')]
release-verify:
    python3 scripts/release.py verify

# Verify the cache, tag vX.Y.Z, push it, move origin/release; GitHub publishes the release
[group('release')]
[arg('version', pattern='[0-9]+\.[0-9]+\.[0-9]+', help='Semver release, e.g. 0.3.1')]
[confirm("This will verify cached Nix outputs, tag, push the tag, force-update origin/release and publish a GitHub release. Continue?")]
release-tag version:
    python3 scripts/release.py tag {{quote(version)}}
