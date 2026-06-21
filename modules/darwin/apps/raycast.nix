{ hostConfig, lib, ... }:

lib.mkIf (builtins.elem hostConfig.hostName [ "insignia" "eulogia" ]) {
  homebrew.casks = [ "raycast" ];
}
