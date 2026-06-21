{ lib, ... }:

{
  home.activation.configureRaycastHotkey = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    /usr/bin/defaults write com.raycast.macos raycastGlobalHotkey -string "Command-49"
    /usr/bin/defaults write com.raycast.macos initialSpotlightHotkey -string "Command-49"
    /usr/bin/defaults write com.raycast.macos mainWindow_isMonitoringGlobalHotkeys -bool true
  '';

  home.activation.prepareRaycastImport = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    import_dir="$HOME/.config/raycast-imports"
    /bin/mkdir -p "$import_dir"

    if [ ! -e "$import_dir/.nix-reminder-shown" ]; then
      echo "Raycast settings import is manual: open Raycast and run Import Settings & Data with your private .rayconfig export."
      /usr/bin/touch "$import_dir/.nix-reminder-shown"
    fi
  '';
}
