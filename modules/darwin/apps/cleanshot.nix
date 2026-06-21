{ hostConfig, lib, pkgs, assetsPath, ... }:

let
  isSharedLaptop = builtins.elem hostConfig.hostName [ "insignia" "eulogia" ];
  userName = hostConfig.userName;
  userHome = "/Users/${userName}";
  domain = "pl.maketheweb.cleanshotx";
  dataKeys = [
    "IKPreferencesLast"
    "LAVArecordVideo"
    "LAVAtakeAllInOne"
    "LAVAtakeArea"
    "LAVAtakeFullscreen"
  ];
  dataAsset = key: "${assetsPath}/cleanshot/${key}.json";
in
lib.mkIf isSharedLaptop {
  system.defaults.CustomUserPreferences.${domain} = {
    SUAutomaticallyUpdate = false;
    SUEnableAutomaticChecks = false;
    afterScreenshotActions = [ 0 1 2 ];
    analyticsAllowed = true;
    annotateLastArrowStyle = 0;
    annotateLastHighlightShape = 1;
    annotateLastPixelateStyle = 0;
    annotateLastSaveURL = "${userHome}/Desktop";
    annotateLastTextSize = 30;
    annotateShowMoreGradients = true;
    annotateTextStyle = 0;
    autoClosePopup = true;
    captureWithoutDesktopIcons = true;
    deletePopupAfterDragging = true;
    markupLastToolIndex = 10;
    markupLastWidth = 1;
    popupAskForDestinationWhenSaving = false;
    popupAutoCloseInterval = 5;
    popupSize = 2;
    shouldShowHorizontalScrollingCaptureGuide = true;
    showMenubarIcon = false;
    snapInAnnotateCrop = true;
    transparentWindowBackground = false;
  };

  system.activationScripts.configureCleanShot.text = ''
    set -euo pipefail

    if [ ! -d ${lib.escapeShellArg userHome} ]; then
      echo "CleanShot user home ${userHome} does not exist; skipping settings restore"
    else
      write_json_data_default() {
        key="$1"
        json_file="$2"
        hex="$(${pkgs.jq}/bin/jq -cj . "$json_file" | /usr/bin/xxd -p -c 256 | /usr/bin/tr -d '\n')"
        /usr/bin/sudo -u ${lib.escapeShellArg userName} /usr/bin/defaults write ${domain} "$key" -data "$hex"
      }

      ${lib.concatMapStringsSep "\n" (key:
        "write_json_data_default ${lib.escapeShellArg key} ${lib.escapeShellArg (dataAsset key)}"
      ) dataKeys}
    fi
  '';
}
