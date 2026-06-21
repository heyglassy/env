{ hostConfig, lib, ... }:

lib.mkIf (builtins.elem hostConfig.hostName [ "insignia" "eulogia" ]) {
  homebrew.casks = [ "flux-app" ];

  system.defaults.CustomUserPreferences."org.herf.Flux" = {
    lateColorTemp = 1400;
    nightColorTemp = 5500;
    transitionSpeed = 0;
    wakeTime = 360;
    locationType = 1;
    disableFullscreen = true;
    darkTheme = true;
  };
}
