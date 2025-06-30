# Justfile
host := `scutil --get ComputerName 2>/dev/null \
         || scutil --get HostName 2>/dev/null \
         || hostname`

update :
    nix flake update

activate :
    darwin-rebuild activate

doctor :
    darwin-rebuild doctor

shells:
    sudo sh -c 'echo "/run/current-system/sw/bin/bash" >> /etc/shells'

switch:
    darwin-rebuild switch --flake .#{{host}}

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
