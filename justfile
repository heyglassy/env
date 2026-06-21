# Justfile
host := `scutil --get ComputerName 2>/dev/null \
         || scutil --get HostName 2>/dev/null \
         || hostname`

update:
    nix flake update

build target=host:
    nix build .#darwinConfigurations.{{target}}.system --no-link

switch target=host:
    @if [ "$(id -u)" -eq 0 ]; then \
        echo "Run this without sudo: just switch {{target}}"; \
        echo "The recipe prepares this user-owned repo for root, then sudo-runs darwin-rebuild."; \
        exit 1; \
    fi
    sudo git config --global --add safe.directory "$PWD"
    sudo darwin-rebuild switch --flake .#{{target}}

full-switch target=host:
    just switch {{target}}
    just post-switch

fswitch target=host:
    just full-switch {{target}}
    just install-final-cut

post-switch:
    bun ./scripts/post_switch.ts
    just kbd

bootstrap:
    just bootstrap-tools
    just bootstrap-apps
    just bootstrap-editors

bootstrap-tools:
    ./scripts/bootstrap_dev_tools.sh

bootstrap-apps:
    ./scripts/install_codex_app.sh

bootstrap-editors:
    ./scripts/install_editor_extensions.sh

install-final-cut:
    @if [ -d "/Applications/Final Cut Pro.app" ]; then \
        echo "Final Cut Pro already installed"; \
        exit 0; \
    fi; \
    available_kb="$(df -Pk / | awk 'NR == 2 { print $4 }')"; \
    required_kb=25000000; \
    if [ "$available_kb" -lt "$required_kb" ]; then \
        echo "Final Cut Pro needs more free space before installing."; \
        echo "Available: $((available_kb / 1024 / 1024)) GiB; recommended minimum: $((required_kb / 1024 / 1024)) GiB."; \
        echo "Free space, then run: just install-final-cut"; \
        exit 1; \
    fi; \
    mas install 424389933

clean-final-cut-temp:
    @tmp_items="${TMPDIR%/}/TemporaryItems"; \
    if [ ! -d "$tmp_items" ]; then \
        exit 0; \
    fi; \
    find "$tmp_items" -maxdepth 1 -type d -name 'NSIRD_mas_*' -exec sh -c ' \
        for dir do \
            if find "$dir" -maxdepth 1 \( -name "424389933-*.pkg" -o -name "424389933-receipt" \) | grep -q .; then \
                rm -rf "$dir"; \
                echo "Removed $dir"; \
            fi; \
        done \
    ' sh {} +

activate:
    darwin-rebuild activate

doctor:
    darwin-rebuild doctor

shells:
    sudo sh -c 'echo "/run/current-system/sw/bin/bash" >> /etc/shells'

audit target=host:
    nix build .#darwinConfigurations.{{target}}.system --no-link
    nix build .#npm-global-tools --no-link
    bunx tsc --noEmit

test-insignia:
    nix build .#insignia --no-link

build-insignia:
    nix build .#insignia --no-link

raycast-import:
    mkdir -p "$HOME/.config/raycast-imports"
    open -a Raycast
    @echo "Run Raycast: Import Settings & Data, then select your private .rayconfig export."

adopt-flux:
    defaults read org.herf.Flux

reset-flux:
    defaults delete org.herf.Flux || true
    darwin-rebuild switch --flake .#{{host}}

test:
    op read "op://Personal/GitHub/public key"

kbd:
    @echo "Attempting to quit Raycast..."
    @pkill -x Raycast || true

    @echo "Waiting for Raycast to exit completely..."
    @while pgrep -x Raycast > /dev/null; do \
        sleep 0.1; \
    done

    @echo "Restarting Raycast..."
    @open -a "Raycast"
    @echo "Raycast has been restarted."
