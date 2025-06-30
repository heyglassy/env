# Justfile
host := `scutil --get ComputerName 2>/dev/null \
         || scutil --get HostName 2>/dev/null \
         || hostname`

switch:
    darwin-rebuild switch --flake .#{{host}}
