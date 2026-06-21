{ lib, ... }:

{
  home.activation.configureRectangleKeybindings = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    /usr/bin/defaults write com.knollsoft.Rectangle alternateDefaultShortcuts -bool true
    /usr/bin/defaults write com.knollsoft.Rectangle launchOnLogin -bool true
    /usr/bin/defaults write com.knollsoft.Rectangle hideMenubarIcon -bool false
    /usr/bin/defaults write com.knollsoft.Rectangle subsequentExecutionMode -int 1
    /usr/bin/defaults write com.knollsoft.Rectangle landscapeSnapAreas -string '[6,{"action":13},1,{"action":15},2,{"action":2},4,{"compound":-2},7,{"compound":-4},5,{"compound":-3},3,{"action":16},8,{"action":14}]'
    /usr/bin/defaults write com.knollsoft.Rectangle toggleTodo -dict \
      keyCode -int 11 \
      modifierFlags -int 786432
    /usr/bin/defaults write com.knollsoft.Rectangle reflowTodo -dict \
      keyCode -int 45 \
      modifierFlags -int 786432
    /usr/bin/defaults write com.knollsoft.Rectangle internalTilingNotified -bool true
    /usr/bin/defaults write com.knollsoft.Rectangle SUEnableAutomaticChecks -bool false
  '';
}
