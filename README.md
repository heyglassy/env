# macOS Setup With Nix, nix-darwin, Home Manager, and Homebrew

This repo is my multi-machine macOS configuration. It installs system tools,
Homebrew apps, Mac App Store apps, shell/editor settings, fonts, and app defaults
for the supported hosts.

Supported hosts:

- `insignia`: personal laptop, user `heyglassy`
- `eulogia`: work laptop, user `eulogia`

The intended new-machine flow is:

1. Install Apple's command-line tools.
2. Install Nix with the Determinate Systems installer.
3. Clone this repo.
4. Run one host-specific nix-darwin switch.
5. Run the repo bootstrap tasks.

## 0. Before You Start

Sign in to these apps first if you want the first switch to install everything
without stopping:

- App Store, with ownership of `Things`.
- 1Password, so Git/JJ signing and SSH agent integration can work after switch.

Create the macOS account with the short name expected by the host:

- `heyglassy` for `insignia`
- `eulogia` for `eulogia`

Do not run `sudo just switch`. The `just` recipe runs `sudo darwin-rebuild`
itself. Running the whole recipe as root makes Git reject this user-owned repo.

## 1. Install Apple's Command-Line Tools

```bash
xcode-select --install
```

If macOS says the tools are already installed, continue.

## 2. Install Nix

Use the Determinate Systems installer:

```bash
curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix \
  | sh -s -- install
```

Close and reopen Terminal, then verify Nix:

```bash
nix run nixpkgs#hello
```

## 3. Clone This Repo

```bash
mkdir -p ~/.config
git clone git@github.com:heyglassy/nix.git ~/.config/nix-darwin
cd ~/.config/nix-darwin
```

If SSH auth is not ready yet, use HTTPS for the first clone:

```bash
git clone https://github.com/heyglassy/nix.git ~/.config/nix-darwin
cd ~/.config/nix-darwin
```

## 4. Pick The Host

Use the host that matches the machine:

```bash
# Personal laptop
host=insignia
```

```bash
# Work laptop
host=eulogia
```

The host name should match the flake target. If this is a fresh Mac, set the
computer name before the first switch:

```bash
sudo scutil --set ComputerName "$host"
sudo scutil --set HostName "$host"
sudo scutil --set LocalHostName "$host"
```

## 5. First nix-darwin Switch

`darwin-rebuild` is not installed yet, so run it through the upstream
`nix-darwin` flake for the first activation:

```bash
sudo git config --global --add safe.directory "$PWD"
sudo nix run nix-darwin/master#darwin-rebuild -- switch --flake ".#$host"
```

This first switch installs:

- nix-darwin and Home Manager activation.
- `just`, `jj`, `mas`, `bun`, `mise`, and other system packages.
- Homebrew through `nix-homebrew`.
- Declared Homebrew formulas and casks.
- Mac App Store apps declared for the host.
- Shell, Git, JJ, SSH, Ghostty, Codex, editor, font, Raycast, Flux, and CleanShot configuration.

## 6. Finish The Bootstrap

After the first switch, start a new shell or open a new terminal window so the
new PATH is loaded.

Register the Nix-managed Bash in `/etc/shells`, then make it your login shell:

```bash
just shells
chsh -s /run/current-system/sw/bin/bash
```

Run the remaining non-critical bootstrap tasks:

```bash
just bootstrap
```

That installs or reconciles:

- Codex CLI from `https://chatgpt.com/codex/install.sh`
- Claude Code
- Can
- Codex desktop app
- VS Code and Cursor extensions
- `mise` tool versions

## 7. Everyday Commands

```bash
# Apply this repo to the current host. Do not prefix this with sudo.
just switch

# Apply a specific host.
just switch insignia
just switch eulogia

# Full workflow: switch, post-switch tasks, and guarded Final Cut install.
just fswitch

# Build without activating.
just build

# Build system and npm tools, then type-check scripts.
just audit

# Run post-switch tasks only.
just post-switch

# Activate the previous built generation.
just activate

# nix-darwin diagnostics.
just doctor
```

Final Cut Pro is intentionally not part of normal `just switch`. It is a large
App Store payload and is not needed for ordinary config rollbacks. Use:

```bash
just install-final-cut
just clean-final-cut-temp
```

## 8. Private Or Manual State

Some state should not live in this repo:

- Raycast settings use Raycast Cloud Sync.
- Berkeley Mono is licensed. Put font files on each machine at:

  ```bash
  ~/.config/nix/darwin-private/fonts-berkeley-mono/
  ```

  On a fresh install, `switch` creates that directory if the font files are not
  present yet. You can also open it with:

  ```bash
  just private-font-dir
  ```

  To restore the fonts from 1Password, upload a zip of the licensed font files
  as a 1Password Document titled `Berkeley Mono Fonts` in the `Personal` vault,
  then run:

  ```bash
  just install-berkeley-mono
  just switch
  ```

  If you use a different document title or vault:

  ```bash
  BERKELEY_MONO_1P_DOCUMENT="Berkeley Mono" \
  BERKELEY_MONO_1P_VAULT="Private" \
  just install-berkeley-mono
  ```

  The activation also checks these older equivalent private locations:

  ```bash
  ~/.config/nix/darwin-private/fonts/berkeley-mono/
  ~/.config/nix-darwin-private/fonts/berkeley-mono/
  ```

- App logins, OAuth sessions, Bluetooth pairing, app licenses, and local secrets
  stay machine-local or in 1Password.

## 9. Updating

```bash
cd ~/.config/nix-darwin
git pull
just update
just switch
```

## 10. Useful One-Offs

```bash
# Open Raycast and remind you to import manually if needed.
just raycast-import

# Inspect current Flux defaults.
just adopt-flux

# Reset Flux defaults back to the repo version.
just reset-flux

# See the nix-darwin changelog.
darwin-rebuild changelog --flake .#insignia
```

## References

- [Determinate Systems Nix for macOS](https://determinate.systems/nix/macos/overview/)
- [Determinate Nix Installer](https://github.com/DeterminateSystems/nix-installer)
- [nix-darwin](https://github.com/nix-darwin/nix-darwin)
- [nix-darwin options](https://nix-darwin.github.io/nix-darwin/manual/)
- [macOS defaults reference](https://macos-defaults.com/)
