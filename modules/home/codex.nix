{ pkgs, lib, hostConfig, ... }:

let
  codexHome = "/Users/${hostConfig.userName}/.codex";
  notifyApp =
    "${codexHome}/computer-use/Codex Computer Use.app/Contents/SharedSupport/SkyComputerUseClient.app/Contents/MacOS/SkyComputerUseClient";

  managedMainConfig = pkgs.writeText "codex-config-managed.toml" ''
    # BEGIN NIX MANAGED CODEX CONFIG
    personality = "pragmatic"
    model = "gpt-5.5"
    model_reasoning_effort = "high"
    plan_mode_reasoning_effort = "high"
    approval_policy = "never"
    sandbox_mode = "danger-full-access"
    approvals_reviewer = "user"
    notify = [${builtins.toJSON notifyApp}, "turn-ended"]
    service_tier = "priority"

    [notice]
    hide_full_access_warning = true
    fast_default_opt_out = true
    hide_rate_limit_model_nudge = true

    [features]
    multi_agent = true
    terminal_resize_reflow = true
    goals = true
    js_repl = false
    memories = false
    chronicle = false

    [desktop]
    appearanceLightCodeThemeId = "linear"
    appearanceDarkCodeThemeId = "nord"
    reviewDelivery = "detached"
    preventSleepWhileRunning = true
    keepRemoteControlAwakeWhilePluggedIn = true
    notifications-turn-mode = "unfocused"
    notifications-permissions-enabled = true
    notifications-questions-enabled = true
    conversationDetailMode = "STEPS_COMMANDS"
    dock-icon-preference = "app-default"

    [desktop.appearanceLightChromeTheme]
    accent = "#5e6ad2"
    contrast = 45
    ink = "#1b1b1b"
    opaqueWindows = true
    surface = "#fcfcfd"

    [desktop.appearanceLightChromeTheme.fonts]
    ui = "Inter"

    [desktop.appearanceLightChromeTheme.semanticColors]
    diffAdded = "#52a450"
    diffRemoved = "#c94446"
    skill = "#8160d8"

    [desktop.appearanceDarkChromeTheme]
    accent = "#88c0d0"
    contrast = 50
    ink = "#d8dee9"
    opaqueWindows = true
    surface = "#2e3440"

    [desktop.appearanceDarkChromeTheme.fonts]
    code = "Berkeley Mono"

    [desktop.appearanceDarkChromeTheme.semanticColors]
    diffAdded = "#a3be8c"
    diffRemoved = "#bf616a"
    skill = "#b48ead"

    [desktop.open-in-target-preferences]
    global = "fileManager"

    [memories]
    generate_memories = false
    use_memories = false
    # END NIX MANAGED CODEX CONFIG
  '';

  managedBrowserConfig = pkgs.writeText "codex-browser-config-managed.toml" ''
    # BEGIN NIX MANAGED CODEX BROWSER CONFIG
    full_cdp_access_enabled = true
    # END NIX MANAGED CODEX BROWSER CONFIG
  '';

  mergeCodexToml = pkgs.writeText "merge-codex-toml.py" ''
import pathlib
import re
import sys
import tomllib

target = pathlib.Path(sys.argv[1])
managed_path = pathlib.Path(sys.argv[2])
mode = sys.argv[3]

configs = {
    "main": {
        "begin": "# BEGIN NIX MANAGED CODEX CONFIG",
        "end": "# END NIX MANAGED CODEX CONFIG",
        "top_keys": {
            "personality",
            "model",
            "model_reasoning_effort",
            "plan_mode_reasoning_effort",
            "approval_policy",
            "sandbox_mode",
            "approvals_reviewer",
            "notify",
            "service_tier",
        },
        "sections": {
            "notice",
            "features",
            "desktop",
            "desktop.appearanceLightChromeTheme",
            "desktop.appearanceLightChromeTheme.fonts",
            "desktop.appearanceLightChromeTheme.semanticColors",
            "desktop.appearanceDarkChromeTheme",
            "desktop.appearanceDarkChromeTheme.fonts",
            "desktop.appearanceDarkChromeTheme.semanticColors",
            "desktop.open-in-target-preferences",
            "memories",
        },
    },
    "browser": {
        "begin": "# BEGIN NIX MANAGED CODEX BROWSER CONFIG",
        "end": "# END NIX MANAGED CODEX BROWSER CONFIG",
        "top_keys": {"full_cdp_access_enabled"},
        "sections": set(),
    },
}

config = configs[mode]
managed = managed_path.read_text().strip() + "\n"
existing = target.read_text() if target.exists() else ""

section_re = re.compile(r"^\s*\[([^\[\]]+)\]\s*(?:#.*)?$")
top_key_re = re.compile(r"^\s*([A-Za-z0-9_-]+)\s*=")

lines = existing.splitlines()
kept = []
in_managed_block = False
skip_section = False
current_section = None

for line in lines:
    if line.strip() == config["begin"]:
        in_managed_block = True
        continue
    if in_managed_block:
        if line.strip() == config["end"]:
            in_managed_block = False
        continue

    section_match = section_re.match(line)
    if section_match:
        current_section = section_match.group(1).strip()
        skip_section = current_section in config["sections"]
        if skip_section:
            continue
        kept.append(line)
        continue

    if skip_section:
        continue

    if current_section is None:
        key_match = top_key_re.match(line)
        if key_match and key_match.group(1) in config["top_keys"]:
            continue

    kept.append(line)

remaining = "\n".join(kept).strip()
new_text = managed + ("\n" + remaining + "\n" if remaining else "")

tomllib.loads(new_text)

old_text = existing
if old_text != new_text:
    if target.exists():
        backup = target.with_name(target.name + ".backup-before-nix-codex")
        backup.write_text(old_text)
    target.write_text(new_text)
  '';
in
{
  home.activation.configureCodex = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -euo pipefail

    mkdir -p "$HOME/.codex/browser"

    ${pkgs.python3}/bin/python3 ${lib.escapeShellArg mergeCodexToml} \
      "$HOME/.codex/config.toml" \
      ${lib.escapeShellArg managedMainConfig} \
      main

    ${pkgs.python3}/bin/python3 ${lib.escapeShellArg mergeCodexToml} \
      "$HOME/.codex/browser/config.toml" \
      ${lib.escapeShellArg managedBrowserConfig} \
      browser
  '';
}
