{ lib, ... }:

{
  home.activation.disableSpotlightHotkeys = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    tmp_plist="$(/usr/bin/mktemp)"
    /usr/bin/defaults export com.apple.symbolichotkeys "$tmp_plist" 2>/dev/null || \
      /usr/bin/plutil -create xml1 "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Delete :AppleSymbolicHotKeys:64" "$tmp_plist" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64 dict" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64:enabled bool false" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64:value dict" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64:value:type string standard" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64:value:parameters array" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64:value:parameters:0 integer 65535" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64:value:parameters:1 integer 49" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:64:value:parameters:2 integer 1048576" "$tmp_plist"

    /usr/libexec/PlistBuddy -c "Delete :AppleSymbolicHotKeys:65" "$tmp_plist" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65 dict" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65:enabled bool false" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65:value dict" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65:value:type string standard" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65:value:parameters array" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65:value:parameters:0 integer 65535" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65:value:parameters:1 integer 49" "$tmp_plist"
    /usr/libexec/PlistBuddy -c "Add :AppleSymbolicHotKeys:65:value:parameters:2 integer 1572864" "$tmp_plist"

    /usr/bin/defaults import com.apple.symbolichotkeys "$tmp_plist"
    /bin/rm -f "$tmp_plist"
    /usr/bin/killall SystemUIServer 2>/dev/null || true
  '';

  home.activation.configureRaycastHotkey = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    /usr/bin/defaults write com.raycast.macos raycastGlobalHotkey -string "Command-49"
    /usr/bin/defaults write com.raycast.macos initialSpotlightHotkey -string "Command-49"
    /usr/bin/defaults write com.raycast.macos mainWindow_isMonitoringGlobalHotkeys -bool true
    /usr/bin/defaults write com.raycast.macos navigationCommandStyleIdentifierKey -string "vim"
    /usr/bin/defaults write com.raycast.macos raycast_hyperKey_state -dict \
      enabled -bool true \
      includeShiftKey -bool true \
      keyCode -int 57
    /usr/bin/defaults write com.raycast.macos useHyperKeyIcon -bool true
    /usr/bin/defaults write com.raycast.macos onboarding_setupHotkey -bool true
    /usr/bin/defaults write com.raycast.macos onboarding_setupAlias -bool true
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
