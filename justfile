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

qs target=host:
    just switch {{target}}
    just kbd

full-switch target=host:
    just switch {{target}}
    just post-switch

fswitch target=host:
    just full-switch {{target}}
    just install-berkeley-mono
    just switch {{target}}
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

private-font-dir:
    mkdir -p "$HOME/.config/nix/darwin-private/fonts-berkeley-mono"
    open "$HOME/.config/nix/darwin-private/fonts-berkeley-mono"
    @echo "Copy licensed BerkeleyMono*.otf/.ttf files here, then run: just switch"

install-berkeley-mono:
    ./scripts/install_berkeley_mono_fonts.sh

install target:
    @if [ "{{target}}" = "final-cut" ]; then \
        just install-final-cut; \
    else \
        echo "Unknown install target: {{target}}"; \
        exit 1; \
    fi

install-final-cut:
    /bin/bash ./scripts/install_final_cut.sh

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

test-glassterm:
    nix build .#glassterm --no-link

build-glassterm:
    nix build .#glassterm --no-link

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
    @for attempt in 1 2 3 4 5; do \
        if open -a "Raycast"; then \
            break; \
        fi; \
        if [ "$attempt" -eq 5 ]; then \
            echo "Raycast did not restart; open it manually."; \
            exit 0; \
        fi; \
        sleep 0.5; \
    done
    @echo "Raycast has been restarted."
