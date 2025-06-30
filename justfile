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
