{ lib, ... }:

{
  home.activation.prepareRaycastImport = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    import_dir="$HOME/.config/raycast-imports"
    mkdir -p "$import_dir"

    if [ ! -e "$import_dir/.nix-reminder-shown" ]; then
      echo "Raycast settings import is manual: open Raycast and run Import Settings & Data with your private .rayconfig export."
      touch "$import_dir/.nix-reminder-shown"
    fi
  '';
}
