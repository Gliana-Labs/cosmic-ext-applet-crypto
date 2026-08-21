#!/usr/bin/env bash
# Install the markets plugin into the local cbar plugin directory.
set -euo pipefail

plugin_dir="${CBAR_PLUGIN_DIR:-$HOME/.config/cbar/plugins}"
src="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/plugins/markets.60s.sh"

for cmd in curl jq; do
  command -v "$cmd" >/dev/null || { printf 'Missing required command: %s\n' "$cmd" >&2; exit 1; }
done

mkdir -p "$plugin_dir"
install -m 0755 "$src" "$plugin_dir/markets.60s.sh"

printf 'Installed: %s/markets.60s.sh\n' "$plugin_dir"
printf 'Configure optional settings in %s/.config/cbar/env\n' "$HOME"
