{ pkgs, hostConfig, ... }:

{
  nix.enable = false;
  nix.settings.allowed-users = [ hostConfig.userName ];

  system.stateVersion = 6;
  system.primaryUser = hostConfig.userName;
  networking.hostName = hostConfig.hostName;

  security.pam.services.sudo_local.touchIdAuth = true;

  users.users.${hostConfig.userName} = {
    home = "/Users/${hostConfig.userName}";
    shell = pkgs.bashInteractive;
  };

  environment.systemPackages = builtins.attrValues {
    inherit (pkgs)
      coreutils
      gnupg
      pinentry_mac
      just
      nodejs_24
      bun
      mas
      wget
      uv
      rustup
      direnv
      jujutsu
      jjui
      cmake
      gnumake
      mise
      duckdb
      tmux;
  };
}
