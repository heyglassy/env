{
  lib,
  rustPlatform,
  bash,
  cctools,
  fetchFromGitHub,
  fixDarwinDylibNames,
  gitMinimal,
  ncurses,
  pkg-config,
  stdenv,
  xcbuild,
  zig_0_15,
}:

let
  ghosttySource = fetchFromGitHub {
    owner = "ghostty-org";
    repo = "ghostty";
    rev = "fdbf9ff3a31d7531b691cb49c98fc465a1a503a0";
    hash = "sha256-TW2dtJ1wZGtdyqQ4YAsfjbTLURhMISIMNK0c0aIy1xM=";
  };
in
rustPlatform.buildRustPackage {
  pname = "insignia";
  version = "0.1.0";

  src = ../tools/insignia;
  cargoHash = "sha256-enmwkUZfGm+KFeDLlzdOeTuwxU8ueteKNTj2V2/CsGk=";

  nativeBuildInputs = [
    gitMinimal
    ncurses
    pkg-config
    zig_0_15
  ]
  ++ lib.optionals stdenv.hostPlatform.isDarwin [
    cctools
    fixDarwinDylibNames
    xcbuild
  ];

  GHOSTTY_SOURCE_DIR = ghosttySource;
  INSIGNIA_TEST_SHELL = "${bash}/bin/bash";

  preBuild = ''
    export ZIG_GLOBAL_CACHE_DIR="$TMPDIR/zig-global-cache"
  '';

  cargoTestFlags = [
    "--bins"
  ];

  dontUseZigConfigure = true;
  dontUseZigBuild = true;
  dontUseZigCheck = true;
  dontUseZigInstall = true;

  meta = {
    description = "Single-pane terminal wrapper with a Ghostty-rendered bottom status bar";
    mainProgram = "insignia";
  };
}
