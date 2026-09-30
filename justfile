# hail — command runner

set shell := ["bash", "-euo", "pipefail", "-c"]

[private]
default:
    @just --list

# All checks: shell lint, release-script tests, tmux scenario harness
[group('check')]
check: lint test-release test

# bash -n and shellcheck over the script, the harness and the scripts
[group('check')]
lint:
    bash -n bin/hail test/run.sh scripts/test-home-manager-module.sh
    shellcheck -S warning bin/hail test/run.sh scripts/test-home-manager-module.sh

# Scenario harness on a scratch tmux server (-L hailtest); never touches yours
[group('check')]
test:
    bash test/run.sh

# Unit tests for scripts/release.py
[group('check')]
test-release:
    python3 scripts/test_release.py

# Evaluate the Home Manager module against stub options
[group('check')]
test-home-manager-module:
    bash scripts/test-home-manager-module.sh

# Build the Nix package and print its version
[group('build')]
build:
    out="$(nix build --no-link --print-out-paths .)" && "$out/bin/hail" --version

# Set the version in bin/hail and scaffold the CHANGELOG entry
[group('release')]
[arg('version', pattern='[0-9]+\.[0-9]+\.[0-9]+', help='Semver release, e.g. 0.3.1')]
release-bump version:
    python3 scripts/release.py bump {{quote(version)}}

# Release readiness: versions agree, changelog filled, clean tree, checks, Nix build
[group('release')]
release-verify:
    python3 scripts/release.py verify

# Tag vX.Y.Z, push it, move origin/release to it; GitHub publishes the release
[group('release')]
[arg('version', pattern='[0-9]+\.[0-9]+\.[0-9]+', help='Semver release, e.g. 0.3.1')]
[confirm("This will tag, push the tag, force-update origin/release and publish a GitHub release. Continue?")]
release-tag version:
    python3 scripts/release.py tag {{quote(version)}}
