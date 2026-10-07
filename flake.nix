{
  description = "hail — agent-to-agent messaging over tmux: envelope in the pane, body in the inbox, receipts, await";
  # Built packages are published to this cache by .github/workflows/nix-cache.yml,
  # so consumers substitute instead of compiling.
  nixConfig = {
    extra-substituters = [ "https://flowerornament.cachix.org" ];
    extra-trusted-public-keys = [
      "flowerornament.cachix.org-1:gSODgIXgfRANrEGITBOF8XWaEKNy8hkNGfRVwqUG46c="
    ];
  };

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      # nixpkgs 26.11 dropped x86_64-darwin; the GitHub release still ships a binary for it.
      systems = [ "aarch64-darwin" "aarch64-linux" "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f {
        pkgs = nixpkgs.legacyPackages.${system};
      });
    in {
      # Version comes from Cargo.toml (see package.nix).
      packages = forAllSystems ({ pkgs }: {
        default = pkgs.callPackage ./package.nix { };
      });

      apps = forAllSystems ({ pkgs }: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/hail";
        };
      });

      homeManagerModules.default = import ./nix/home-manager.nix {
        inherit self;
        src = ./.;
      };

      # Source tree path for skill syncing (nix-config agent-sync.nix).
      skillsDir = "${self}/skills";
    };
}
