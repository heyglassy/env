#!/usr/bin/env bash
set -euo pipefail

app_id="424389933"
required_kb=25000000

find_mas() {
  for candidate in \
    /run/current-system/sw/bin/mas \
    /opt/homebrew/bin/mas \
    /usr/local/bin/mas; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  command -v mas 2>/dev/null
}

for app_path in \
  "/Applications/Final Cut Pro.app" \
  "/Applications/Final Cut.app" \
  "$HOME/Applications/Final Cut Pro.app" \
  "$HOME/Applications/Final Cut.app"; do
  if [ -d "$app_path" ]; then
    echo "Final Cut already installed at $app_path"
    exit 0
  fi
done

if /usr/bin/mdfind "kMDItemAppStoreAdamID == $app_id" 2>/dev/null | /usr/bin/grep -q .; then
  echo "Final Cut already installed"
  exit 0
fi

mas_bin="$(find_mas || true)"
if [ -z "$mas_bin" ]; then
  echo "mas is not installed; run just switch first."
  exit 1
fi

if "$mas_bin" list | /usr/bin/awk '{ print $1 }' | /usr/bin/grep -qx "$app_id"; then
  echo "Final Cut already installed according to Mac App Store"
  exit 0
fi

available_kb="$(/bin/df -Pk / | /usr/bin/awk 'NR == 2 { print $4 }')"
if [ "$available_kb" -lt "$required_kb" ]; then
  echo "Final Cut needs more free space before installing."
  echo "Available: $((available_kb / 1024 / 1024)) GiB; recommended minimum: $((required_kb / 1024 / 1024)) GiB."
  echo "Free space, then run: just install-final-cut"
  exit 1
fi

"$mas_bin" install "$app_id"
