# Declarative macOS Setup with Nix, nix-darwin & home-manager

This guide shows how to get a fully declarative macOS configuration on Apple Silicon
using:

- **Nix**: functional package manager
- **nix-darwin**: declarative macOS system config
- **home-manager**: user-level dotfiles & tools
- **Flakes**: reproducible, composable Nix configs

Once set up, standing up a new Mac is:

1. Install Nix
2. `git clone` your flake repo
3. One `darwin-rebuild switch`

## Prerequisites

- Apple Silicon MacBook Pro (M1/M2/M3)
- Xcode command-line tools:
  ```bash
  xcode-select --install
  ```

## 1. Install Nix

Recommended: use the Determinate Systems installer (enables flakes, is uninstallable, survives OS updates):

```bash
curl --proto '=https' --tlsv1.2 -sSf \
  -L https://install.determinate.systems/nix \
| sh -s -- install
```

Close and re-open your terminal. Verify:

```bash
nix run "nixpkgs#hello"   # prints "Hello, world!"
```

## 1 Clone and bootstrap (no `just` yet)

```bash
# Pick any location you like. ~/.config/nix is conventional.
git clone <repo>
cd <repo>

# get userName
whoami

# get hostName
scutil --get ComputerName 2>/dev/null \
         || scutil --get HostName 2>/dev/null \
         || hostname

nix run nix-darwin -- switch --flake .

just shells # to setup the shells
chsh -s /run/current-system/sw/bin/bash # has to be run manually for some reason
```

What the bootstrap does:

- installs / upgrades Nix (Determinate Systems installer)
- applies every nix-darwin module in `flake.nix`
- activates Home-Manager for the current user
- drops **`just`** plus all declared packages into your `PATH`

When the prompt returns you are on the fully declarative system and can use the
short `just` aliases below.

---

## 2 Everyday commands (now that `just` exists)

```bash
# Re-apply the current flake to the system
just switch            # = darwin-rebuild switch --flake .#$(hostname)

# Edit flake files and test without activating
just build             # = darwin-rebuild build  --flake .#$(hostname)

# Roll back to the previous generation (undo a bad config)
just activate          # = darwin-rebuild activate

# Garbage-collect everything but the last 3 generations
just gc

# Diagnostics
just doctor            # = nix doctor
```

---

## 3 Keeping everything up to date

```bash
# 1. Pull the latest version of this repo
git -C ~/.config/nix pull

# 2. Update flake inputs (nixpkgs, home-manager, nix-darwin …)
just update            # = nix flake update

# 3. Build the new system
just switch
```

---

## 4 Useful one-offs

```bash
# See the changelog between current and next nix-darwin release
darwin-rebuild changelog --flake .#insignia
```

## References

[Setting up Nix on macOS](https://nixcademy.com/posts/nix-on-macos/)
[NixOS Package Search](https://search.nixos.org/packages?channel=unstable&show=go&from=0&size=1&sort=relevance&type=packages&query=go)
[Nix Darwin Configuration Options](https://nix-darwin.github.io/nix-darwin/manual/)

## Note

- You can set a lot of defaults for MacOS via Nix ( https://macos-defaults.com/ )
