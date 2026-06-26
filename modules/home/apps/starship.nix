{ assetsPath, hostConfig, ... }:

let
  baseConfig = builtins.readFile (assetsPath + "/starship.toml");
  eulogiaConfig =
    builtins.replaceStrings
      [
        ''Macos = "\uE400"''
        "format = '[ insigna ]($style)'"
      ]
      [
        ''Macos = "\uE401"''
        "format = '[ $user ]($style)'"
      ]
      baseConfig;
in

{
  programs.starship = {
    enable = true;
    enableBashIntegration = false;
  };
  xdg.configFile."starship.toml".text =
    if hostConfig.hostName == "eulogia" then eulogiaConfig else baseConfig;
  xdg.configFile."starship-glassterm.toml".source =
    assetsPath + "/starship-glassterm.toml";
}
