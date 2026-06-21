{
  description = "Glassy's Nix Darwin + Home-Manager configuration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    nix-darwin.url = "github:LnL7/nix-darwin";
    nix-darwin.inputs.nixpkgs.follows = "nixpkgs";

    nix-homebrew.url = "github:zhaofengli/nix-homebrew";

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, nix-darwin, home-manager, nix-homebrew, ... }@inputs:
    let
      hosts = {
        insignia = import ./hosts/insignia.nix;
        eulogia = import ./hosts/eulogia.nix;
      };

      mkPkgs = system:
        import nixpkgs {
          inherit system;
          config = {
            allowUnfree = true;
            allowBroken = true;
          };
          overlays = [];
        };

      mkDarwinConfiguration = hostConfig:
        let
          pkgs = mkPkgs hostConfig.system;
        in
        nix-darwin.lib.darwinSystem {
          inherit pkgs;
          system = hostConfig.system;
          specialArgs = {
            inherit inputs self hostConfig;
            assetsPath = ./assets;
          };
          modules = [
            nix-homebrew.darwinModules.nix-homebrew
            home-manager.darwinModules.home-manager

            ./modules/darwin/core.nix
            ./modules/darwin/system-defaults.nix
            ./modules/darwin/homebrew.nix
            ./modules/darwin/apps/cleanshot.nix
            ./modules/darwin/apps/flux.nix
            ./modules/darwin/apps/raycast.nix

            ({ hostConfig, ... }: {
              home-manager.backupFileExtension = "backup";
              home-manager.useGlobalPkgs = true;
              home-manager.extraSpecialArgs = {
                inherit inputs self hostConfig;
                assetsPath = ./assets;
              };
              home-manager.users.${hostConfig.userName}.imports = [
                ./modules/home/core.nix
                ./modules/home/shell.nix
                ./modules/home/git.nix
                ./modules/home/jj.nix
                ./modules/home/ssh.nix
                ./modules/home/atuin.nix
                ./modules/home/codex.nix
                ./modules/home/editors.nix
                ./modules/home/npm-tools.nix
                ./modules/home/apps/direnv.nix
                ./modules/home/apps/fonts.nix
                ./modules/home/apps/ghostty.nix
                ./modules/home/apps/raycast.nix
                ./modules/home/apps/rectangle.nix
                ./modules/home/apps/starship.nix
              ];
            })
          ];
        };

      defaultSystem = "aarch64-darwin";
      defaultPkgs = mkPkgs defaultSystem;
    in
    {
      darwinConfigurations =
        builtins.mapAttrs (_name: hostConfig: mkDarwinConfiguration hostConfig) hosts;

      packages.${defaultSystem} = {
        default = self.darwinConfigurations.insignia.system;
        insignia = defaultPkgs.callPackage ./packages/insignia.nix {};
        npm-global-tools = defaultPkgs.callPackage ./packages/npm-global-tools.nix {};
      };
    };
}
