{ assetsPath, ... }:

{
  programs.jujutsu = {
    enable = true;
    settings = builtins.fromTOML (builtins.readFile (assetsPath + "/jj_config.toml"));
  };
}
