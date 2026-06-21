#!/usr/bin/env bash
set -euo pipefail

app_id="424389933"
bundle_id="com.apple.FinalCut"
required_kb=25000000

final_cut_installed() {
  for app_path in \
    "/Applications/Final Cut Pro.app" \
    "/Applications/Final Cut.app" \
    "$HOME/Applications/Final Cut Pro.app" \
    "$HOME/Applications/Final Cut.app"; do
    if app_is_final_cut "$app_path"; then
      echo "Final Cut already installed at $app_path"
      return 0
    fi
  done

  for applications_dir in /Applications "$HOME/Applications"; do
    [ -d "$applications_dir" ] || continue
    for app_path in "$applications_dir"/*.app; do
      [ -e "$app_path" ] || continue
      if app_is_final_cut "$app_path"; then
        echo "Final Cut already installed at $app_path"
        return 0
      fi
    done
  done

  while IFS= read -r app_path; do
    if app_is_final_cut "$app_path"; then
      echo "Final Cut already installed at $app_path"
      return 0
    fi
  done < <(/usr/bin/mdfind "kMDItemCFBundleIdentifier == '$bundle_id'" 2>/dev/null)

  while IFS= read -r app_path; do
    if app_is_final_cut "$app_path"; then
      echo "Final Cut already installed at $app_path"
      return 0
    fi
  done < <(/usr/bin/mdfind "kMDItemAppStoreAdamID == $app_id" 2>/dev/null)

  return 1
}

app_is_final_cut() {
  app_path="$1"
  info_plist="$app_path/Contents/Info.plist"

  [ -d "$app_path" ] || return 1
  [ -f "$info_plist" ] || return 1

  app_bundle_id="$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$info_plist" 2>/dev/null || true)"
  [ "$app_bundle_id" = "$bundle_id" ]
}

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

if final_cut_installed; then
  exit 0
fi

mas_bin="$(find_mas || true)"
if [ -z "$mas_bin" ]; then
  echo "mas is not installed; run just switch first."
  exit 1
fi

if final_cut_installed; then
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
