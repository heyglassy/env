{
  description = "Glassy's Nix Darwin + Home-Manager configuration";

  ##############################################################################
  # ─── INPUTS ─────────────────────────────────────────────────────────────────
  ##############################################################################
  inputs = {
    # Unstable nixpkgs for fresh macOS support
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    # Nix Darwin
    nix-darwin.url = "github:LnL7/nix-darwin";
    nix-darwin.inputs.nixpkgs.follows = "nixpkgs";

    # Home-Manager
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  ##############################################################################
  # ─── OUTPUTS ────────────────────────────────────────────────────────────────
  ##############################################################################
  outputs = { self, nixpkgs, nix-darwin, home-manager, ... }@inputs:
    let
      # Host & user data – adjust if you rename the machine or account
      # hostName = <hostname>;
      # userName = <username>;
      hostName = "insignia";
      userName = "heyglassy";
      system = "aarch64-darwin"; # or "x86_64-darwin" for Intel Macs

      # Import the desired nixpkgs for this system
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true; # remove if you want a fully-free set
        config.allowBroken = true;
      };

      ############################################################################
      # ─── DARWIN MODULE STACK ──────────────────────────────────────────────────
      ############################################################################
      modules = [
        #################################################
        # Core Nix Darwin configuration for the machine
        #################################################
        {
          nix.enable = false;
          system.stateVersion = 6;
          networking.hostName = hostName;
          security.pam.services.sudo_local.touchIdAuth = true;
          system.keyboard.enableKeyMapping = true;
          system.keyboard.swapLeftCtrlAndFn = true;

          # User account recognised by nix-darwin (not strictly required,
          # but convenient if you ever build Nix on a fresh macOS install).
          users.users.${userName} = {
            home = "/Users/${userName}";
            shell = pkgs.zsh;
          };

          # Miscellaneous macOS-specific settings go here …
          # services = { ... };
        }
        {
          environment.systemPackages = with pkgs; [ gnupg pinentry_mac just ];
        }
        {
          system.primaryUser = userName; # userName is the let-binding at the top
          # Tell nix-darwin to bootstrap Homebrew and use it
          homebrew = {
            enable = true;

            # Optional but nice: keep Homebrew itself up-to-date and
            # remove orphaned casks/brews automatically on each switch
            onActivation = {
              autoUpdate = true; # `brew update`
              cleanup = "zap";   # or "uninstall" if you prefer
            };

            taps = [ "homebrew/cask" ]; # cask repo is needed for Edge
            casks = [ "arc" "1password" "legcord" "cursor" "ghostty" "raycast" "beeper" "superhuman" "figma" "notion" "hiddenbar" "cloudflare-warp" "notion-calendar" ];
          };
        }

        #################################################
        # Home-Manager as a nix-darwin module
        #################################################
        {
          # import the official Home-Manager module
          imports = [ home-manager.darwinModules.home-manager ];

          # extra knob you want to tweak
          home-manager.backupFileExtension = "backup";
        }

        #################################################
        # Per-user Home-Manager configuration
        #################################################
        {
          # Enable home-manager for the user
          home-manager.users.${userName} = { pkgs, ... }: {
            home = {
              homeDirectory = "/Users/${userName}";
              stateVersion = "23.11";
            };

            programs.ssh = {
              enable = true;
              extraConfig = ''
                Host *
                  IgnoreUnknown UseKeychain
                  UseKeychain yes
                  IdentityAgent "~/.1password/agent.sock"   # 1Password -> SSH agent
                  AddKeysToAgent yes
              '';
            };
            
            programs.git = {
              enable = true;
              package = pkgs.gitFull; # Git ≥ 2.34 is required for SSH signing

              extraConfig = {
                user.signingKey =
                  "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILO80lHtITf+AgJgfNSVe20l5dwrv9clt9M1dVHZ7W6A";

                gpg.format = "ssh";


                commit.gpgsign = true;
              };
            };

            programs.atuin = {
              enable = true;
              settings = {
                auto_sync = true;
                enter_accept = true;
              };
            };

            programs.bash = {
              enable = true;                    # activates Home-Manager's Bash module
              package = pkgs.bashInteractive;   # this is Bash 5.2 from nixpkgs
            };

            programs.starship = {
              enable = true;
            };
            xdg.configFile."starship.toml".source = ./starship.toml;

            xdg.configFile."ghostty/config".source = ./ghostty_config.txt;
          };
        }
      ];
    in
    {
      # The entry that `darwin-rebuild`/`just switch` points at
      darwinConfigurations.${hostName} =
        nix-darwin.lib.darwinSystem {
          inherit system pkgs modules;
        };

      # `nix build` without any attribute gives you a closure to copy elsewhere
      packages.${system}.default =
        self.darwinConfigurations.${hostName}.system;
    };
}