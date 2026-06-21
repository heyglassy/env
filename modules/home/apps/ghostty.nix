{ pkgs, assetsPath, hostConfig, self, ... }:

let
  backgroundName =
    if hostConfig.hostName == "eulogia" then "eulogia.png" else "insignia.png";
  backgroundPath =
    "/Users/${hostConfig.userName}/.config/ghostty/backgrounds/${backgroundName}";
  insignia = self.packages.${pkgs.stdenv.hostPlatform.system}.insignia;
in
{
  home.packages = [
    insignia
  ];

  xdg.configFile."ghostty/config".text =
    (builtins.replaceStrings
      [ "title = insignia" ]
      [ "title = ${hostConfig.hostName}" ]
      (builtins.readFile (assetsPath + "/ghostty_config.txt"))) + ''

      command = ${insignia}/bin/insignia -- /run/current-system/sw/bin/bash -l
      background-image = ${backgroundPath}
      background-image-opacity = 0.08
      background-image-position = center
      background-image-fit = contain
      background-image-repeat = false
    '';

  xdg.configFile."ghostty/backgrounds/insignia.png".source =
    assetsPath + "/ghostty/backgrounds/insignia.png";
  xdg.configFile."ghostty/backgrounds/eulogia.png".source =
    assetsPath + "/ghostty/backgrounds/eulogia.png";
}
