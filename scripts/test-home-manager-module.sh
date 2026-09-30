#!/usr/bin/env bash
# Evaluate the exported Home Manager module against stub options and prove:
# the bare case installs the producer package, the configured case exports
# HAIL_ENVELOPE_MAX and links the skill, a package override is honoured, and
# the assertions refuse skill.enable without enable.
set -euo pipefail

for cmd in nix git python3; do
  command -v "$cmd" >/dev/null 2>&1 || { printf 'error: missing required command: %s\n' "$cmd" >&2; exit 1; }
done

ROOT="$(git rev-parse --show-toplevel)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/hail-hm.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# Copy the tracked tree so the flake evaluates from a clean path.
SOURCE="$WORK/source"; mkdir -p "$SOURCE"
(cd "$ROOT" && git ls-files -z | tar --null -T - -cf -) | tar -xf - -C "$SOURCE"

EVAL="$WORK/eval.nix"
cat > "$EVAL" <<'NIX'
{ root, mode }:
let
  flake = builtins.getFlake "path:${root}";
  pkgs = import flake.inputs.nixpkgs { system = builtins.currentSystem; };
  lib = flake.inputs.nixpkgs.lib;
  module = flake.outputs.homeManagerModules.default;
  producer = flake.outputs.packages.${builtins.currentSystem}.default;
  override = pkgs.writeShellScriptBin "hail-test-override" "exit 0";
  stub = { lib, ... }: {
    options.assertions = lib.mkOption { type = lib.types.listOf lib.types.attrs; default = [ ]; };
    options.home.packages = lib.mkOption { type = lib.types.listOf lib.types.package; default = [ ]; };
    options.home.sessionVariables = lib.mkOption { type = lib.types.attrsOf lib.types.str; default = { }; };
    options.home.file = lib.mkOption { type = lib.types.attrsOf lib.types.attrs; default = { }; };
  };
  caseModule =
    if mode == "bare" then { programs.hail.enable = true; }
    else if mode == "configured" then {
      programs.hail.enable = true;
      programs.hail.envelopeMax = 240;
      programs.hail.skill.enable = true;
      programs.hail.skill.targets = [ ".agents/skills/hail" ".claude/skills/hail" ];
    }
    else if mode == "override" then { programs.hail.enable = true; programs.hail.package = override; }
    else if mode == "skill-without-enable" then { programs.hail.skill.enable = true; }
    else if mode == "absolute-target" then {
      programs.hail.enable = true; programs.hail.skill.enable = true;
      programs.hail.skill.targets = [ "/abs/skills/hail" ];
    }
    else throw "unknown mode ${mode}";
  evaluated = lib.evalModules { modules = [ module stub caseModule ]; specialArgs.pkgs = pkgs; };
  cfg = evaluated.config;
in {
  assertionsOk = builtins.all (a: a.assertion) cfg.assertions;
  packageCount = builtins.length cfg.home.packages;
  packageDrv = if cfg.home.packages == [ ] then null else (builtins.head cfg.home.packages).drvPath;
  producerDrv = producer.drvPath;
  overrideDrv = override.drvPath;
  envelopeMax = cfg.home.sessionVariables.HAIL_ENVELOPE_MAX or null;
  skillTargets = builtins.attrNames cfg.home.file;
  skillSourcesEndInSkill = builtins.all (f: lib.hasSuffix "/skills/hail" (toString f.source)) (builtins.attrValues cfg.home.file);
}
NIX

evaluate() { nix eval --impure --json --expr "import \"$EVAL\" { root = \"$SOURCE\"; mode = \"$1\"; }"; }
bare="$(evaluate bare)"; configured="$(evaluate configured)"; override="$(evaluate override)"
skill_without_enable="$(evaluate skill-without-enable)"
if evaluate absolute-target >/dev/null 2>&1; then
  echo "absolute skill target unexpectedly evaluated" >&2; exit 1
fi

python3 - "$bare" "$configured" "$override" "$skill_without_enable" <<'PY'
import json, sys
bare, configured, override, skill_without_enable = (json.loads(a) for a in sys.argv[1:])
def check(cond, msg):
    if not cond: raise SystemExit(f"home-manager smoke test: {msg}")
check(bare["assertionsOk"], "bare case tripped an assertion")
check(bare["packageCount"] == 1, "bare case did not install exactly one package")
check(bare["packageDrv"] == bare["producerDrv"], "bare case did not install the producer package")
check(bare["envelopeMax"] is None, "bare case exported HAIL_ENVELOPE_MAX")
check(bare["skillTargets"] == [], "bare case linked skills")
check(configured["assertionsOk"], "configured case tripped an assertion")
check(configured["envelopeMax"] == "240", "configured case did not export HAIL_ENVELOPE_MAX=240")
check(sorted(configured["skillTargets"]) == [".agents/skills/hail", ".claude/skills/hail"], "configured case did not link both skill targets")
check(configured["skillSourcesEndInSkill"], "skill links do not point at skills/hail")
check(override["packageDrv"] == override["overrideDrv"], "package override was not installed")
check(not skill_without_enable["assertionsOk"], "skill.enable without enable was accepted")
print("bare=producer configured=env+skill override=honoured assertions=enforced")
PY
printf 'Home Manager module smoke test passed.\n'
