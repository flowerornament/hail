{
  description = "hail — agent-to-agent messaging over tmux: envelope in the pane, body in the inbox, receipts, await";
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      hailVersion = "0.2.1";
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f {
        pkgs = nixpkgs.legacyPackages.${system};
      });
    in {
      packages = forAllSystems ({ pkgs }: {
        default = pkgs.callPackage ./package.nix { };
      });

      homeManagerModules.default = import ./nix/home-manager.nix {
        inherit self;
        src = ./.;
      };

      # Source tree path for skill syncing (nix-config agent-sync.nix).
      skillsDir = "${self}/skills";
    };
}
