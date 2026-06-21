{ ... }:

{
  system.keyboard.enableKeyMapping = true;
  system.keyboard.swapLeftCtrlAndFn = true;

  system.defaults.NSGlobalDomain = {
    AppleICUForce24HourTime = true;
    AppleInterfaceStyle = "Dark";
    AppleShowAllExtensions = true;
    AppleShowScrollBars = "Always";
    AppleSpacesSwitchOnActivate = false;
    NSAutomaticCapitalizationEnabled = true;
    NSAutomaticDashSubstitutionEnabled = true;
    NSAutomaticPeriodSubstitutionEnabled = true;
    NSAutomaticQuoteSubstitutionEnabled = true;
    NSAutomaticSpellingCorrectionEnabled = true;
    "com.apple.springing.enabled" = true;
    "com.apple.springing.delay" = 0.5;
    "com.apple.trackpad.forceClick" = true;
    "com.apple.trackpad.scaling" = 0.875;
  };

  system.defaults.dock = {
    autohide = true;
    largesize = 128;
    launchanim = true;
    magnification = false;
    minimize-to-application = true;
    mru-spaces = false;
    show-process-indicators = true;
    show-recents = false;
    tilesize = 48;
    persistent-apps = [];
    persistent-others = [];
    wvous-br-corner = 1;
    wvous-tr-corner = 1;
  };

  system.defaults.finder = {
    AppleShowAllExtensions = true;
    AppleShowAllFiles = false;
    FXDefaultSearchScope = "SCcf";
    FXPreferredViewStyle = "Nlsv";
    ShowPathbar = true;
    ShowStatusBar = true;
  };

  system.defaults.screencapture = {
    show-thumbnail = false;
    type = "png";
  };

  system.defaults.trackpad = {
    Clicking = false;
    DragLock = false;
    Dragging = false;
    TrackpadCornerSecondaryClick = 0;
    TrackpadFourFingerHorizSwipeGesture = 2;
    TrackpadFourFingerPinchGesture = 2;
    TrackpadFourFingerVertSwipeGesture = 2;
    TrackpadMomentumScroll = true;
    TrackpadPinch = true;
    TrackpadRightClick = true;
    TrackpadRotate = true;
    TrackpadThreeFingerDrag = false;
    TrackpadThreeFingerHorizSwipeGesture = 2;
    TrackpadThreeFingerTapGesture = 0;
    TrackpadThreeFingerVertSwipeGesture = 2;
    TrackpadTwoFingerDoubleTapGesture = true;
    TrackpadTwoFingerFromRightEdgeSwipeGesture = 3;
  };

  system.defaults.CustomUserPreferences = {
    "com.apple.driver.AppleBluetoothMultitouch.trackpad" = {
      Clicking = 0;
      DragLock = 0;
      Dragging = 0;
      TrackpadCornerSecondaryClick = 0;
      TrackpadFiveFingerPinchGesture = 2;
      TrackpadFourFingerHorizSwipeGesture = 2;
      TrackpadFourFingerPinchGesture = 2;
      TrackpadFourFingerVertSwipeGesture = 2;
      TrackpadHandResting = 1;
      TrackpadHorizScroll = 1;
      TrackpadMomentumScroll = 1;
      TrackpadPinch = 1;
      TrackpadRightClick = 1;
      TrackpadRotate = 1;
      TrackpadScroll = 1;
      TrackpadThreeFingerDrag = 0;
      TrackpadThreeFingerHorizSwipeGesture = 2;
      TrackpadThreeFingerTapGesture = 0;
      TrackpadThreeFingerVertSwipeGesture = 2;
      TrackpadTwoFingerDoubleTapGesture = 1;
      TrackpadTwoFingerFromRightEdgeSwipeGesture = 3;
      USBMouseStopsTrackpad = 0;
      UserPreferences = 1;
    };

    "com.apple.AppleMultitouchTrackpad" = {
      ActuateDetents = 1;
      ActuationStrength = 0;
      Clicking = 0;
      DragLock = 0;
      Dragging = 0;
      FirstClickThreshold = 1;
      ForceSuppressed = 0;
      SecondClickThreshold = 1;
      TrackpadCornerSecondaryClick = 0;
      TrackpadFiveFingerPinchGesture = 2;
      TrackpadFourFingerHorizSwipeGesture = 2;
      TrackpadFourFingerPinchGesture = 2;
      TrackpadFourFingerVertSwipeGesture = 2;
      TrackpadHandResting = 1;
      TrackpadHorizScroll = 1;
      TrackpadMomentumScroll = 1;
      TrackpadPinch = 1;
      TrackpadRightClick = 1;
      TrackpadRotate = 1;
      TrackpadScroll = 1;
      TrackpadThreeFingerDrag = 0;
      TrackpadThreeFingerHorizSwipeGesture = 2;
      TrackpadThreeFingerTapGesture = 0;
      TrackpadThreeFingerVertSwipeGesture = 2;
      TrackpadTwoFingerDoubleTapGesture = 1;
      TrackpadTwoFingerFromRightEdgeSwipeGesture = 3;
      USBMouseStopsTrackpad = 0;
      UserPreferences = 1;
    };

    "NSGlobalDomain" = {
      AppleMiniaturizeOnDoubleClick = 0;
      "com.apple.mouse.scaling" = "0.875";
      "com.apple.trackpad.scrolling" = "0.1838";
    };
  };
}
