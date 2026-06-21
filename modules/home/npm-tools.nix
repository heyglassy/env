{ pkgs, ... }:

let
  npmGlobalTools = pkgs.callPackage ../../packages/npm-global-tools.nix {};
in
{
  home.packages = [ npmGlobalTools ];
}
