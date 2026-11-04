#!/bin/sh
set -eu
app="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
config_file="$HOME/Library/Application Support/KMServer/login-config-dir"
if [ -f "$config_file" ]; then
	config_dir="$(cat "$config_file")"
else
	config_dir="$HOME/.komga"
fi
exec "$app/Contents/MacOS/kmrs" --config-dir "$config_dir"
