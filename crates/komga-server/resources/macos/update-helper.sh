#!/bin/sh
# Invoked by a running KMServer.app after its replacement has been downloaded.
# The staged app lives beside the installed app, so rename is atomic on one volume.
set -eu

old_pid="$1"
app="$2"
staged="$3"
log="$4"
config_dir="$5"
port="$6"
backup="${app}.update-backup"
work="${app}.update-work"

exec >>"$log" 2>&1
printf 'Waiting for KMServer pid %s to exit\n' "$old_pid"
waited=0
while kill -0 "$old_pid" 2>/dev/null; do
	sleep 1
	waited=$((waited + 1))
	if [ "$waited" -ge 90 ]; then
		printf 'KMServer did not stop in time; the installed app is unchanged\n'
		exit 1
	fi
done

[ -d "$app" ]
[ -d "$staged/KMServer.app" ]
[ ! -e "$backup" ]
[ ! -e "$work" ]
codesign --verify --deep --strict "$staged/KMServer.app"
spctl --assess --type execute "$staged/KMServer.app"
ditto "$staged/KMServer.app" "$work"

if ! mv "$app" "$backup"; then
	printf 'Could not preserve the installed app\n'
	exit 1
fi
if mv "$work" "$app"; then
	if open "$app" --args --config-dir "$config_dir"; then
		waited=0
		while [ "$waited" -lt 30 ]; do
			if curl -fsS "http://127.0.0.1:$port/actuator/health" >/dev/null 2>&1; then
				rm -rf "$backup"
				printf 'Update installed\n'
				exit 0
			fi
			sleep 1
			waited=$((waited + 1))
		done
	fi
	printf 'New app could not launch; restoring the previous version\n'
	rm -rf "$app"
else
	printf 'New app could not be installed; restoring the previous version\n'
fi
mv "$backup" "$app"
exit 1
