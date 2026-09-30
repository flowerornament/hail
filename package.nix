# hail: agent-to-agent messaging over tmux (envelope + inbox + receipts + await).
# Flake-free; consume with `callPackage ./path/to/hail/package.nix { }`.
# Installs bin/hail, a `tmux-bridge` compatibility symlink to it, and the agent
# skill under share/hail/skills/hail.
{ lib, stdenvNoCC }:

let
  # The one version source is the VERSION= line in bin/hail: `hail --version`,
  # this package, the flake and the release tooling all read it from there.
  version = builtins.head (builtins.match
    ".*\nVERSION=\"([0-9]+\\.[0-9]+\\.[0-9]+)\"\n.*"
    (builtins.readFile ./bin/hail));
in
stdenvNoCC.mkDerivation {
  pname = "hail";
  inherit version;

  src = ./.;

  dontBuild = true;
  dontConfigure = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 bin/hail $out/bin/hail
    ln -s hail $out/bin/tmux-bridge
    mkdir -p $out/share/hail/skills
    cp -r skills/hail $out/share/hail/skills/hail
    runHook postInstall
  '';

  # `bash -n` on the installed script, with the shebang already patched to the
  # store's bash by fixupPhase.
  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    bash -n $out/bin/hail
    [ "$($out/bin/hail --version)" = "hail ${version}" ]
    [ "$($out/bin/tmux-bridge version)" = "hail ${version}" ]
    $out/bin/hail --help >/dev/null
    runHook postInstallCheck
  '';

  meta = {
    description = "Agent-to-agent messaging over tmux: envelope, inbox, receipts, await";
    mainProgram = "hail";
    license = lib.licenses.mit;
    platforms = lib.platforms.unix;
  };
}
