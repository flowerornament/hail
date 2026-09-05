# hail: agent-to-agent messaging over tmux (envelope + inbox + receipts + await).
# Flake-free; consume with `callPackage ./path/to/hail/package.nix { }`.
# Installs bin/hail, a `tmux-bridge` compatibility symlink to it, and the agent
# skill under share/hail/skills/hail.
{ lib, stdenvNoCC }:

stdenvNoCC.mkDerivation {
  pname = "hail";
  version = "0.2.0";

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
    [ "$($out/bin/hail version)" = "hail 0.2.0" ]
    [ "$($out/bin/tmux-bridge version)" = "hail 0.2.0" ]
    runHook postInstallCheck
  '';

  meta = {
    description = "Agent-to-agent messaging over tmux: envelope, inbox, receipts, await";
    mainProgram = "hail";
    license = lib.licenses.mit;
    platforms = lib.platforms.unix;
  };
}
