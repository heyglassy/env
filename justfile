# Justfile
host := `scutil --get ComputerName 2>/dev/null \
         || scutil --get HostName 2>/dev/null \
         || hostname`

update := nix flake update

activate := darwin-rebuild activate

doctor := darwin-rebuild doctor

switch:
    darwin-rebuild switch --flake .#{{host}}
