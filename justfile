# Justfile
host := `scutil --get ComputerName 2>/dev/null \
         || scutil --get HostName 2>/dev/null \
         || hostname`

user := `if [ -n "$SUDO_USER" ]; then printf "%s" "$SUDO_USER"; else id -un; fi`

cwd := `pwd`

update :
    nix flake update

ghostty-title:
    sed -i '' -E "s/^title[[:space:]]*=[[:space:]]*.*/title = {{host}}/" {{cwd}}/ghostty_config.txt

activate :
    darwin-rebuild activate

doctor :
    darwin-rebuild doctor

shells:
    sudo sh -c 'echo "/run/current-system/sw/bin/bash" >> /etc/shells'

config: ghostty-title
    printf '%s\n' '{' "    hostName = \"{{host}}\";" "    userName = \"{{user}}\";" '}' > configuration.nix

switch: config
    darwin-rebuild switch --flake .#{{host}}
    bun ./scripts/post_switch.ts
    just kbd

test:
    op read "op://Personal/GitHub/public key"

kbd:
    @echo "Attempting to quit Raycast..."
    # Send quit signal. `|| true` prevents an error if it's not running.
    @pkill -x Raycast || true

    @echo "Waiting for Raycast to exit completely..."
    # Loop and wait until the process is no longer found.
    @while pgrep -x Raycast > /dev/null; do \
        sleep 0.1; \
    done

    @echo "Restarting Raycast..."
    @open -a "Raycast"
    @echo "Raycast has been restarted."
