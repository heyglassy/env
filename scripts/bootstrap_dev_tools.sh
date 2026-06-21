#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.local/bin:$HOME/.can/bin:/run/current-system/sw/bin:/opt/homebrew/bin:/usr/local/bin:/bin:/usr/bin:$PATH"

run_installer() {
  local url="$1"
  local shell_bin="$2"
  local tmpdir

  tmpdir="$(mktemp -d)"
  if ! curl -fsSL -o "$tmpdir/install.sh" "$url"; then
    rm -rf "$tmpdir"
    return 1
  fi
  if ! "$shell_bin" "$tmpdir/install.sh"; then
    rm -rf "$tmpdir"
    return 1
  fi
  rm -rf "$tmpdir"
}

install_codex_cli() {
  local tmpdir

  tmpdir="$(mktemp -d)"
  if ! curl -fsSL -o "$tmpdir/install.sh" "https://chatgpt.com/codex/install.sh"; then
    rm -rf "$tmpdir"
    return 1
  fi
  if ! CODEX_NON_INTERACTIVE=true sh "$tmpdir/install.sh"; then
    if [ -x "$HOME/.local/bin/codex" ] && "$HOME/.local/bin/codex" --version >/dev/null 2>&1; then
      echo "Codex CLI installed; skipping installer shell profile update because PATH is managed by Nix."
    else
      rm -rf "$tmpdir"
      return 1
    fi
  fi
  rm -rf "$tmpdir"
}

if command -v mise >/dev/null 2>&1; then
  mise install
else
  echo "mise not found; run just switch first so Nix installs it."
fi

if [ ! -x "$HOME/.can/bin/can" ]; then
  echo "Installing Can..."
  run_installer "https://releases.seltzer.dev/install.sh" sh
else
  echo "Can already installed"
fi

if [ ! -x "$HOME/.local/bin/codex" ]; then
  echo "Installing Codex CLI..."
  install_codex_cli
else
  echo "Codex CLI already installed"
fi

if [ ! -x "$HOME/.local/bin/claude" ]; then
  echo "Installing Claude Code..."
  run_installer "https://claude.ai/install.sh" bash
else
  echo "Claude Code already installed"
fi
