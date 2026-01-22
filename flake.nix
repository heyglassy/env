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

    nix-homebrew.url = "github:zhaofengli/nix-homebrew";


    # Home-Manager
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };

  };

  ##############################################################################
  # ─── OUTPUTS ────────────────────────────────────────────────────────────────
  ##############################################################################
  outputs = { self, nixpkgs, nix-darwin, home-manager, nix-homebrew, ... }@inputs:
    let
      # Host & user data – adjust in `configuration.nix`
      inherit (import ./configuration.nix) hostName userName;

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
          nix.settings.allowed-users = [ userName ];
          system.stateVersion = 6;
          networking.hostName = hostName;
          security.pam.services.sudo_local.touchIdAuth = true;
          system.keyboard.enableKeyMapping = true;
          system.keyboard.swapLeftCtrlAndFn = true;
          system.defaults.dock = {
            autohide = true;
            tilesize = 48;
            persistent-apps = [];
          };

          # User account recognised by nix-darwin (not strictly required,
          # but convenient if you ever build Nix on a fresh macOS install).
          users.users.${userName} = {
            home = "/Users/${userName}";
            shell = pkgs.bashInteractive;
          };

          # Miscellaneous macOS-specific settings go here …
          # services = { ... };
        }
        nix-homebrew.darwinModules.nix-homebrew
        {
          nix-homebrew = {
            # Install Homebrew under the default prefix
            enable = true;

            # Apple Silicon Only: Also install Homebrew under the default Intel prefix for Rosetta 2
            enableRosetta = true;

            # User owning the Homebrew prefix
            user = userName;

            # Automatically migrate existing Homebrew installations
            autoMigrate = true;
          };
        }
        {
          environment.systemPackages = with pkgs; [ coreutils gnupg pinentry_mac just bun fnm wget uv rustup direnv jujutsu jjui cmake mise duckdb zed-editor ];
        }
        {
          system.primaryUser = userName; # userName is the let-binding at the top
          # Tell nix-darwin to bootstrap Homebrew and use it
          homebrew = {
            enable = true;
            user = userName;

            # Optional but nice: keep Homebrew itself up-to-date and
            # remove orphaned casks/brews automatically on each switch
            onActivation = {
              autoUpdate = true; # `brew update`
              cleanup = "zap";   # or "uninstall" if you prefer
            };

            brews = [
              "pscale"
              "go@1.24"
              "temporalio/brew/tcld"
              "withgraphite/tap/graphite"
              "cocoapods"
              "gh"
              "fastlane"
              "bufbuild/buf/buf"
              "protobuf"
              "ripgrep"
              "yt-dlp"
              "ffmpeg"
            ];
            taps = [
              "temporalio/brew"
              "withgraphite/tap"
            ];
            casks = [
              "ngrok"
              "1password-cli"
              "tailscale"
              "orbstack"
              "slack"
              "arc"
              "1password"
              "legcord"
              "cursor"
              "ghostty"
              "raycast"
              "beeper"
              "superhuman"
              "figma"
              "notion"
              "hiddenbar"
              "cloudflare-warp"
              "notion-calendar"
              "rectangle"
              "flux-app"
              "rescuetime"
              "cleanshot"
              "macfuse"
            ];
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
          # Enable home-manager for the use
          home-manager.users.${userName} = { pkgs, lib, ... }: {
            home = {
              homeDirectory = "/Users/${userName}";
              stateVersion = "23.11";

              # Install GNU coreutils and findutils for Home Manager compatibility
              packages = with pkgs; [
                coreutils   # Provides GNU readlink with -e flag
                findutils   # Provides GNU find with -printf support
              ];

              sessionVariables = {
                PATH = "${pkgs.coreutils}/bin:$PATH";
              };

              # Override PATH before linkGeneration to ensure GNU tools are used
              activation.setupGnuTools = lib.hm.dag.entryBefore [ "linkGeneration" ] ''
                PATH="${pkgs.coreutils}/bin:${pkgs.findutils}/bin:$PATH"
              '';

              # Install global bun packages on activation
              activation.installBunGlobalPackages = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
                ${pkgs.bun}/bin/bun install -g opencode-ai qmd
              '';

              # Install TigrisFS for macOS
              activation.installTigrisFS = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
                if ! command -v tigrisfs &> /dev/null; then
                  echo "Installing TigrisFS for macOS..."
                  export PATH="${pkgs.curl}/bin:${pkgs.gnutar}/bin:${pkgs.gzip}/bin:/usr/bin:$PATH"
                  export VERIFY_CHECKSUM=false
                  export INSTALL_DIR="$HOME/.local/bin"
                  mkdir -p "$INSTALL_DIR"
                  ${pkgs.curl}/bin/curl -sSL https://raw.githubusercontent.com/tigrisdata/tigrisfs/refs/heads/main/install.sh | ${pkgs.bashInteractive}/bin/bash
                  echo "TigrisFS installed to $INSTALL_DIR/tigrisfs"
                fi
              '';

              # Install Claude Code using native installer
              activation.installClaudeCode = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
                if ! command -v claude &> /dev/null; then
                  echo "Installing Claude Code..."
                  export PATH="${pkgs.curl}/bin:${pkgs.gnutar}/bin:${pkgs.gzip}/bin:/usr/bin:$PATH"
                  ${pkgs.curl}/bin/curl -fsSL https://claude.ai/install.sh | ${pkgs.bashInteractive}/bin/bash
                  echo "Claude Code installed"
                fi
              '';

            };

            programs.ssh = {
              enable = true;
              enableDefaultConfig = false;
              matchBlocks."*" = {
                extraOptions = {
                  IdentityAgent = "\"~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock\"";
                };
              };
            };
            
            programs.git = {
              enable = true;
              package = pkgs.gitFull; # Git ≥ 2.34 is required for SSH signing
              settings = {
                user.name = "Christian Glassiognon";
                user.email = "63924603+heyglassy@users.noreply.github.com";
                user.signingKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIC/qhn9neDAsXF7tbLp+sao9P1YFq5/2pTIo5L/I5FFU";
                commit.gpgSign = true;
                gpg.format = "ssh";
                gpg.ssh.program = "/Applications/1Password.app/Contents/MacOS/op-ssh-sign";
                push.default = "simple";
                branch.autoSetupMerge = "simple";
              };
            };

            programs.jujutsu = {
              enable = true;
              settings = {
                user.name = "Christian Glassiognon";
                user.email = "63924603+heyglassy@users.noreply.github.com";
                signing = {
                  backend = "ssh";
                  key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIC/qhn9neDAsXF7tbLp+sao9P1YFq5/2pTIo5L/I5FFU";
                  behavior = "own";  # Auto-sign all commits you author
                  backends.ssh.program = "/Applications/1Password.app/Contents/MacOS/op-ssh-sign";
                };
              };
            };

            programs.atuin = {
              enable = true;
              enableBashIntegration = false;  # We'll add our own with TTY check
              settings = {
                auto_sync = true;
                enter_accept = true;
                filter_mode_shell_up_key_binding = "directory";
              };
            };

            programs.bash = {
              enable = true;                    # activates Home-Manager's Bash module
              package = pkgs.bashInteractive;   # this is Bash 5.2 from nixpkgs
              shellAliases = {
                zed = "zeditor";
              };
              initExtra = ''
                export EDITOR=vim
                export VISUAL=vim
                export PATH="/opt/homebrew/bin/pod:$PATH"

                eval "$(fnm env --use-on-cd --shell bash)"
                eval "$(/opt/homebrew/bin/brew shellenv)"
                export PATH="/Users/carnegie/go/bin:$PATH"
                export PATH="/Users/carnegie/kernel/packages/api/bin:$PATH"
                export PATH="/Users/heyglassy/.bun/bin:$PATH"
                export PATH="$HOME/.local/bin:$PATH"

                # Atuin shell history - only init when stdin is a TTY
                if [[ :$SHELLOPTS: =~ :(vi|emacs): ]] && [[ -t 0 ]]; then
                  source "${pkgs.bash-preexec}/share/bash/bash-preexec.sh"
                  eval "$(${pkgs.atuin}/bin/atuin init bash)"
                fi
              '';
            };
            # xdg.configFile."clang".source = ./clang;

            programs.starship = {
              enable = true;
            };

            programs.direnv = {
              enable = true;
              nix-direnv.enable = true;
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
