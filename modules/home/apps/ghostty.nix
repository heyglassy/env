{ pkgs, assetsPath, hostConfig, self, ... }:

let
  backgroundName =
    if hostConfig.hostName == "eulogia" then "eulogia.png" else "insignia.png";
  backgroundPath =
    "/Users/${hostConfig.userName}/.config/ghostty/backgrounds/${backgroundName}";
  insignia = self.packages.${pkgs.stdenv.hostPlatform.system}.insignia;
  appSupportConfigText = ''
    theme = test
  '';
  configText =
    (builtins.replaceStrings
      [ "title = insignia" ]
      [ "title = ${hostConfig.hostName}" ]
      (builtins.readFile (assetsPath + "/ghostty_config.txt"))) + ''

      command = /usr/bin/env -u NO_COLOR INSIGNIA_STATUS_LABEL=${hostConfig.hostName} INSIGNIA_STATUS_FOREGROUND=#f0d77d INSIGNIA_STATUS_BACKGROUND=#1f0f02 INSIGNIA_DISABLE_OSC_THEME_QUERY=1 ${insignia}/bin/insignia -- /run/current-system/sw/bin/bash -l
      background-image = ${backgroundPath}
      background-image-opacity = 0.08
      background-image-position = center
      background-image-fit = contain
      background-image-repeat = false
    '';
in
{
  home.packages = [
    insignia
  ];

  xdg.configFile."ghostty/config".text = configText;
  home.file."Library/Application Support/com.mitchellh.ghostty/config".text = appSupportConfigText;

  xdg.configFile."ghostty/backgrounds/insignia.png".source =
    assetsPath + "/ghostty/backgrounds/insignia.png";
  xdg.configFile."ghostty/backgrounds/eulogia.png".source =
    assetsPath + "/ghostty/backgrounds/eulogia.png";
  xdg.configFile."ghostty/themes/test".source =
    assetsPath + "/ghostty/themes/test";
  home.file."Library/Application Support/com.mitchellh.ghostty/themes/test".source =
    assetsPath + "/ghostty/themes/test";
}
