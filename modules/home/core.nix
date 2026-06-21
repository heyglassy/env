{ pkgs, lib, hostConfig, ... }:

{
  home = {
    homeDirectory = "/Users/${hostConfig.userName}";
    stateVersion = "23.11";

    packages = builtins.attrValues {
      inherit (pkgs) coreutils findutils;
    };

    sessionPath = [
      "$HOME/.nix-profile/bin"
      "/run/current-system/sw/bin"
      "/opt/homebrew/bin"
      "/opt/homebrew/sbin"
      "$HOME/.can/bin"
      "$HOME/.local/bin"
      "$HOME/Library/pnpm"
    ];

    sessionVariables = {
      PATH = "$HOME/.local/bin:$HOME/.can/bin:$HOME/.nix-profile/bin:/run/current-system/sw/bin:${pkgs.nodejs_24}/bin:${pkgs.bun}/bin:${pkgs.coreutils}/bin:/opt/homebrew/bin:/opt/homebrew/sbin:$HOME/Library/pnpm:$PATH";
      PNPM_HOME = "$HOME/Library/pnpm";
    };

    activation.setupGnuTools = lib.hm.dag.entryBefore [ "linkGeneration" ] ''
      PATH="${pkgs.coreutils}/bin:${pkgs.findutils}/bin:$PATH"
    '';

    activation.setupNodePackageDirs = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      mkdir -p "$HOME/Library/pnpm"
    '';

    activation.cleanupLegacyNpmCliShims = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      for path in \
        "$HOME/.local/bin/qmd" \
        "$HOME/.npm-global/bin/codex" \
        "$HOME/.npm-global/bin/ni" \
        "$HOME/.npm-global/bin/nci" \
        "$HOME/.npm-global/bin/nr" \
        "$HOME/.npm-global/bin/nup" \
        "$HOME/.npm-global/bin/nd" \
        "$HOME/.npm-global/bin/nlx" \
        "$HOME/.npm-global/bin/na" \
        "$HOME/.npm-global/bin/nun" \
        "$HOME/.npm-global/bin/qmd" \
        "$HOME/.npm-global/bin/pi"
      do
        if [ -e "$path" ] || [ -L "$path" ]; then
          rm -f "$path"
        fi
      done

      for path in \
        "/opt/homebrew/bin/ni" \
        "/opt/homebrew/bin/nci" \
        "/opt/homebrew/bin/nr" \
        "/opt/homebrew/bin/nup" \
        "/opt/homebrew/bin/nd" \
        "/opt/homebrew/bin/nlx" \
        "/opt/homebrew/bin/na" \
        "/opt/homebrew/bin/nun"
      do
        if [ -L "$path" ]; then
          target="$(readlink "$path")"
          case "$target" in
            *node_modules/@antfu/ni*) rm -f "$path" ;;
          esac
        fi
      done
    '';

  };
}
