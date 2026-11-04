#!/bin/sh
# Invoked by a running KMServer.app after its replacement has been downloaded.
# The staged app lives beside the installed app, so rename is atomic on one volume.
set -eu

old_pid="$1"
app="$2"
update_dir="$3"
config_dir="$4"
port="$5"
# the login item's launchd service, when the server should run under it
service="${6:-}"
staged="$update_dir/staged/KMServer.app"
log="$config_dir/logs/update.log"
backup="${app}.update-backup"
work="${app}.update-work"
new_pid=

cleanup() {
	status=$?
	if [ "$status" -ne 0 ]; then
		printf 'Update failed\n'
	fi
	rm -rf "$update_dir"
	trap - EXIT
	exit "$status"
}
trap cleanup EXIT

launch_and_check() {
	if [ -n "$service" ]; then
		check_service
		return
	fi
	# job control starts the server in its own process group, so it outlives
	# anything that ends this script's group
	set -m
	"$app/Contents/MacOS/kmrs" --config-dir "$config_dir" &
	new_pid=$!
	set +m
	waited=0
	while [ "$waited" -lt 30 ]; do
		kill -0 "$new_pid" 2>/dev/null || {
			wait "$new_pid" || true
			return 1
		}
		if server_healthy; then
			sleep 1
			if kill -0 "$new_pid" 2>/dev/null; then
				return 0
			fi
			wait "$new_pid" || true
			return 1
		fi
		sleep 1
		waited=$((waited + 1))
	done
	kill "$new_pid" 2>/dev/null || true
	wait "$new_pid" || true
	return 1
}

server_healthy() {
	curl -fsS "http://127.0.0.1:$port/actuator/health" 2>/dev/null | grep -q '"status"[[:space:]]*:[[:space:]]*"UP"'
}

# launchd starts the server, so it relaunches it after a crash; -k replaces a
# failed new server that is still running when the previous app is restored
check_service() {
	launchctl kickstart -k "$service" || return 1
	waited=0
	while [ "$waited" -lt 30 ]; do
		if server_healthy; then
			sleep 1
			server_healthy && return 0
		fi
		sleep 1
		waited=$((waited + 1))
	done
	return 1
}

restore_running_app() {
	if [ -n "$new_pid" ]; then
		kill "$new_pid" 2>/dev/null || true
		wait "$new_pid" 2>/dev/null || true
	fi
	if [ -d "$backup" ]; then
		rm -rf "$app"
		mv "$backup" "$app" || return 1
	fi
	if launch_and_check; then
		printf 'Previous installation restored\n'
		return 0
	fi
	printf 'Previous installation could not restart\n'
	return 1
}

exec >>"$log" 2>&1
printf 'Waiting for KMServer pid %s to exit\n' "$old_pid"
waited=0
while kill -0 "$old_pid" 2>/dev/null && ! ps -p "$old_pid" -o stat= | grep -q '^Z'; do
	sleep 1
	waited=$((waited + 1))
	if [ "$waited" -ge 90 ]; then
		printf 'KMServer did not stop in time; the installed app is unchanged\n'
		open "$app" --args --config-dir "$config_dir" || true
		exit 1
	fi
done

[ -d "$app" ] || {
	restore_running_app
	exit 1
}
[ -d "$staged" ] || {
	restore_running_app
	exit 1
}
[ ! -e "$backup" ] || {
	restore_running_app
	exit 1
}
[ ! -e "$work" ] || {
	restore_running_app
	exit 1
}
codesign --verify --deep --strict "$staged" || {
	restore_running_app
	exit 1
}
spctl --assess --type execute "$staged" || {
	restore_running_app
	exit 1
}
ditto "$staged" "$work" || {
	restore_running_app
	exit 1
}

if ! mv "$app" "$backup"; then
	printf 'Could not preserve the installed app\n'
	restore_running_app
	exit 1
fi
if mv "$work" "$app"; then
	if launch_and_check; then
		rm -rf "$backup"
		printf 'Update installed\n'
		exit 0
	fi
	printf 'New app could not launch; restoring the previous version\n'
else
	printf 'New app could not be installed; restoring the previous version\n'
fi
restore_running_app
