# hail: messages between coding agents on one machine (envelope + inbox +
# receipts + await). Flake-free; consume with `callPackage ./path/to/hail/package.nix { }`.
# Installs bin/hail, a `tmux-bridge` compatibility symlink to it, and the agent
# skill under share/hail/skills/hail.
{ lib, rustPlatform }:

let
  # The one version source is Cargo.toml: `hail --version`, this package, the
  # flake and the release tooling all read it from there.
  version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
in
rustPlatform.buildRustPackage {
  pname = "hail";
  inherit version;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./tests
      ./skills
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  # The tmux scenarios (test/run.sh) need a tmux server; the cargo tests do not.
  doCheck = true;

  postInstall = ''
    ln -s hail $out/bin/tmux-bridge
    mkdir -p $out/share/hail/skills
    cp -r skills/hail $out/share/hail/skills/hail
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    [ "$($out/bin/hail --version)" = "hail ${version}" ]
    [ "$($out/bin/tmux-bridge version)" = "hail ${version}" ]
    $out/bin/hail --help >/dev/null
    runHook postInstallCheck
  '';

  meta = {
    description = "Messages between coding agents on one machine: envelope, inbox, receipts, await";
    mainProgram = "hail";
    license = lib.licenses.mit;
    platforms = lib.platforms.unix;
  };
}
