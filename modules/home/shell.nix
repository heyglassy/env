{ pkgs, ... }:

{
  programs.bash = {
    enable = true;
    package = pkgs.bashInteractive;
    enableCompletion = false;
    shellOptions = [ "histappend" "extglob" ];
    shellAliases = {};
    profileExtra = ''
      export PATH="$HOME/.local/bin:$HOME/.can/bin:$HOME/.nix-profile/bin:/run/current-system/sw/bin:${pkgs.nodejs_24}/bin:${pkgs.bun}/bin:/opt/homebrew/bin:/opt/homebrew/sbin:$HOME/Library/pnpm:$PATH"
      # Non-interactive login shells return early from ~/.bashrc, so activate mise here too.
      case $- in
        *i*) ;;
        *) command -v mise >/dev/null 2>&1 && eval "$(mise activate bash)" ;;
      esac
    '';
    bashrcExtra = ''
      shopt -s globstar 2>/dev/null || true
      shopt -s checkjobs 2>/dev/null || true
    '';
    initExtra = ''
      if [[ -z ''${BASH_COMPLETION_VERSINFO+x} ]]; then
        . "${pkgs.bash-completion}/etc/profile.d/bash_completion.sh"
      fi

      export EDITOR=vim
      export VISUAL=vim
      export PATH="$HOME/.local/bin:$HOME/.can/bin:$HOME/.nix-profile/bin:/run/current-system/sw/bin:/opt/homebrew/bin:/opt/homebrew/sbin:$HOME/Library/pnpm:$PATH"
      export PATH="$HOME/.bun/bin:$PATH"
      export PATH="$HOME/.raindrop/bin:$PATH"

      # Activate mise after PATH mutations so its shims stay first.
      command -v mise >/dev/null 2>&1 && eval "$(mise activate bash)"
      export PATH="$HOME/.local/bin:$HOME/.can/bin:$HOME/.nix-profile/bin:/run/current-system/sw/bin:${pkgs.nodejs_24}/bin:${pkgs.bun}/bin:/opt/homebrew/bin:/opt/homebrew/sbin:$HOME/Library/pnpm:$PATH"

      # Atuin shell history - only init when stdin is a TTY
      if [[ :$SHELLOPTS: =~ :(vi|emacs): ]] && [[ -t 0 ]]; then
        source "${pkgs.bash-preexec}/share/bash/bash-preexec.sh"
        eval "$(${pkgs.atuin}/bin/atuin init bash)"
      fi

      if [ -f "$HOME/.bashrc.local" ]; then
        source "$HOME/.bashrc.local"
      fi

      if [[ -n "''${GLASSTERM:-}" ]]; then
        export STARSHIP_CONFIG="$HOME/.config/starship-glassterm.toml"
      fi

      eval "$(${pkgs.direnv}/bin/direnv hook bash)"

      if [[ $TERM != "dumb" ]]; then
        eval "$(${pkgs.starship}/bin/starship init bash --print-full-init)"
      fi

      if [[ -n "''${GLASSTERM:-}" ]]; then
        __glassterm_urlencode_pwd() {
          local i ch encoded=""
          local LC_ALL=C
          for ((i = 0; i < ''${#PWD}; i++)); do
            ch="''${PWD:i:1}"
            case "$ch" in
              [a-zA-Z0-9.~_/-]) encoded+="$ch" ;;
              *) printf -v ch '%%%02X' "'$ch"; encoded+="$ch" ;;
            esac
          done
          printf '%s' "$encoded"
        }

        __glassterm_emit_osc7() {
          local host="''${HOSTNAME:-}"
          if [[ -z "$host" ]]; then
            host="$(hostname 2>/dev/null || printf localhost)"
          fi
          printf '\033]7;file://%s%s\033\\' "$host" "$(__glassterm_urlencode_pwd)"
        }

        if declare -p PROMPT_COMMAND >/dev/null 2>&1 &&
          [[ "$(declare -p PROMPT_COMMAND)" == declare\ -a* ]]; then
          PROMPT_COMMAND+=(__glassterm_emit_osc7)
        elif [[ -n "''${PROMPT_COMMAND:-}" ]]; then
          PROMPT_COMMAND="''${PROMPT_COMMAND};__glassterm_emit_osc7"
        else
          PROMPT_COMMAND="__glassterm_emit_osc7"
        fi
      fi
    '';
  };
}
