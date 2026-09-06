{ self, src }:
{ config, lib, pkgs, ... }:
let
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
  isRelativeSkillTarget = target: target != "" && builtins.substring 0 1 target != "/";
in
{
  options.programs.hail = {
    enable = lib.mkEnableOption "hail agent messaging";

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "hail package from this flake";
      description = "The hail package to install (provides `hail` and the `tmux-bridge` alias).";
    };

    envelopeMax = lib.mkOption {
      type = lib.types.nullOr lib.types.int;
      default = null;
      example = 240;
      description = "Maximum envelope length typed into a pane (HAIL_ENVELOPE_MAX). Default in the tool is 240.";
    };

    skill = {
      enable = lib.mkEnableOption "hail skill symlink management";
      targets = lib.mkOption {
        type = lib.types.listOf (lib.types.addCheck lib.types.str isRelativeSkillTarget);
        default = [ ".agents/skills/hail" ];
        example = lib.literalExpression ''[ ".agents/skills/hail" ".claude/skills/hail" ]'';
        description = "Home-relative paths where Home Manager should symlink hail's `skills/hail` directory.";
      };
    };
  };

  config =
    let
      cfg = config.programs.hail;
      hasUniqueSkillTargets = builtins.length cfg.skill.targets == builtins.length (lib.unique cfg.skill.targets);
      skillFiles = lib.genAttrs cfg.skill.targets (_: { source = "${src}/skills/hail"; });
    in
    lib.mkMerge [
      {
        assertions = [
          { assertion = !cfg.skill.enable || cfg.enable;
            message = "programs.hail.skill.enable requires programs.hail.enable = true"; }
          { assertion = !cfg.skill.enable || hasUniqueSkillTargets;
            message = "programs.hail.skill.targets must not contain duplicate paths"; }
        ];
      }
      (lib.mkIf cfg.enable { home.packages = [ cfg.package ]; })
      (lib.mkIf (cfg.enable && cfg.envelopeMax != null) {
        home.sessionVariables.HAIL_ENVELOPE_MAX = toString cfg.envelopeMax;
      })
      (lib.mkIf (cfg.enable && cfg.skill.enable) { home.file = skillFiles; })
    ];
}
