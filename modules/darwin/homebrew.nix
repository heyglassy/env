{ hostConfig, lib, ... }:

let
  isInsignia = hostConfig.hostName == "insignia";
  isEulogia = hostConfig.hostName == "eulogia";
  isSharedLaptop = isInsignia || isEulogia;

  sharedLaptopCasks = [
    "tailscale-app"
    "ghostty"
    "figma"
    "helium-browser"
    "orbstack"
    "cleanshot"
    "rectangle"
    "wispr-flow"
    "notion-calendar"
    "monodraw"
    "1password-cli"
  ];

  insigniaOnlyCasks = [
    "slack"
    "arc"
    "1password"
    "legcord"
    "cursor"
    "zed"
    "beeper"
    "superhuman"
    "notion"
    "hiddenbar"
    "cloudflare-warp"
    "rescuetime"
    "visual-studio-code"
    "linear"
  ];

  sharedLaptopMasApps = {
    "Things" = 904280696;
  };
in

{
  nix-homebrew = {
    enable = true;
    enableRosetta = true;
    user = hostConfig.userName;
    autoMigrate = true;
  };

  homebrew = {
    enable = true;
    user = hostConfig.userName;
    prefix = "/opt/homebrew";

    onActivation = {
      autoUpdate = false;
      cleanup = "zap";
    };

    brews = [
      "pscale"
      "go@1.24"
      "temporalio/brew/tcld"
      "withgraphite/tap/graphite"
      "cloudflare-wrangler"
      "cocoapods"
      "gh"
      "fastlane"
      "bufbuild/buf/buf"
      "protobuf"
      "ripgrep"
      "yt-dlp"
      "ffmpeg"
      "abseil"
      "ca-certificates"
      "certifi"
      "dav1d"
      "deno"
      "go"
      "jpeg-turbo"
      "lame"
      "libtiff"
      "libvpx"
      "libyaml"
      "little-cms2"
      "lz4"
      "mpdecimal"
      "mole"
      "openssl@3"
      "opus"
      "pcre2"
      "python@3.14"
      "readline"
      "ruby"
      "sdl2"
      "sqlite"
      "svt-av1"
      "terminal-notifier"
      "x264"
      "x265"
      "xz"
      "zstd"
    ];

    taps = [
      "temporalio/brew"
      "withgraphite/tap"
    ];

    casks =
      (lib.optionals isSharedLaptop sharedLaptopCasks)
      ++ (lib.optionals isInsignia insigniaOnlyCasks);

    masApps = lib.optionalAttrs isSharedLaptop sharedLaptopMasApps;
  };
}
