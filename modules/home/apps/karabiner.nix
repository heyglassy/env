{ assetsPath, hostConfig, lib, ... }:

let
  karabinerConfig = assetsPath + "/karabiner/karabiner.json";
in
lib.mkIf (hostConfig.hostName == "eulogia") {
  home.activation.configureKarabiner = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    config_dir="$HOME/.config/karabiner"
    config_file="$config_dir/karabiner.json"

    /bin/mkdir -p "$config_dir"
    /usr/bin/install -m 600 "${karabinerConfig}" "$config_file"
  '';
}
