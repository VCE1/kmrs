#!/bin/bash
# Test the helper with isolated fake apps and local HTTP health checks.
set -euo pipefail

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
OLD_MODE="${OLD_MODE:-old}"
STAGED_MODE="${STAGED_MODE:-new}"
HELPER_SCRIPT="${HELPER_SCRIPT:-$ROOT/crates/komga-server/resources/macos/update-helper.sh}"
WORK="$(mktemp -d)"
APP="$WORK/KMServer.app"
UPDATE_DIR="$WORK/update"
STAGED="$UPDATE_DIR/staged"
CONFIG="$WORK/config"
PORT="${TEST_PORT:-28793}"
OLD_PID=
NEW_PID=

cleanup() {
	[ -z "$OLD_PID" ] || kill "$OLD_PID" 2>/dev/null || true
	[ -z "$NEW_PID" ] || kill "$NEW_PID" 2>/dev/null || true
	rm -rf "$WORK"
}
trap cleanup EXIT

mkdir -p "$APP/Contents/MacOS" "$STAGED/KMServer.app/Contents/MacOS" "$CONFIG/logs" "$WORK/tools"
for tool in codesign spctl; do
	printf '#!/bin/sh\nexit 0\n' >"$WORK/tools/$tool"
	chmod +x "$WORK/tools/$tool"
done
cat >"$WORK/tools/ditto" <<'EOF'
#!/bin/sh
exec /bin/cp -R "$1" "$2"
EOF
chmod +x "$WORK/tools/ditto"
cat >"$WORK/server.py" <<'EOF'
import http.server
import os
import pathlib
import sys

mode, _, config_dir = sys.argv[1:]
root = pathlib.Path(os.environ["TEST_WORK"])
if mode == "restore-fail":
    (root / "restored-start").write_text("yes")
    sys.exit(1)
if mode == "fail":
    (root / "failed-start").write_text("yes")
    sys.exit(1)
(root / f"{mode}-config").write_text(config_dir)
(root / f"{mode}-pid").write_text(str(os.getpid()))

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        status = 200 if self.path == "/actuator/health" else 404
        self.send_response(status)
        self.end_headers()
        if status == 200:
            self.wfile.write(b'{"status":"UP"}')
    def log_message(self, *args):
        pass

http.server.HTTPServer(("127.0.0.1", int(os.environ["TEST_PORT"])), Handler).serve_forever()
EOF

write_app() {
	local app="$1" mode="$2"
	cat >"$app/Contents/MacOS/kmrs" <<EOF
#!/bin/sh
exec /usr/bin/python3 -I "$WORK/server.py" $mode "\$@"
EOF
	chmod +x "$app/Contents/MacOS/kmrs"
}
write_app "$APP" old
write_app "$STAGED/KMServer.app" "$STAGED_MODE"
if [ "$OLD_MODE" = restore-fail ]; then
	write_app "$STAGED/KMServer.app" fail
fi
export TEST_WORK="$WORK" TEST_PORT="$PORT"
export PATH="$WORK/tools:$PATH"

"$APP/Contents/MacOS/kmrs" --config-dir "$CONFIG" &
OLD_PID=$!
for _ in $(seq 1 30); do
	curl -fsS "http://127.0.0.1:$PORT/actuator/health" >/dev/null 2>&1 && break
	sleep .1
done
kill "$OLD_PID"
wait "$OLD_PID" || true
HELPER_FAILED=0
if [ "$OLD_MODE" = restore-fail ]; then
	write_app "$APP" restore-fail
fi
sh "$HELPER_SCRIPT" \
	"$OLD_PID" "$APP" "$UPDATE_DIR" "$CONFIG" "$PORT" || HELPER_FAILED=$?
[ ! -e "$UPDATE_DIR" ]

if [ "$OLD_MODE" = restore-fail ]; then
	[ "$HELPER_FAILED" -ne 0 ]
	[ -f "$WORK/restored-start" ]
	printf 'helper failed recovery test passed\n'
	exit 0
fi
[ "$HELPER_FAILED" -eq 0 ]

if [ "$STAGED_MODE" = fail ]; then
	for _ in $(seq 1 50); do
		[ -f "$WORK/old-pid" ] && break
		sleep .1
	done
	[ -f "$WORK/old-pid" ]
	kill -0 "$(cat "$WORK/old-pid")"
	[ "$(cat "$WORK/old-config")" = "$CONFIG" ]
	curl -fsS "http://127.0.0.1:$PORT/actuator/health" >/dev/null
	NEW_PID="$(cat "$WORK/old-pid")"
else
	for _ in $(seq 1 50); do
		[ -f "$WORK/new-pid" ] && break
		sleep .1
	done
	[ -f "$WORK/new-pid" ]
	[ "$(cat "$WORK/new-config")" = "$CONFIG" ]
	curl -fsS "http://127.0.0.1:$PORT/actuator/health" >/dev/null
	NEW_PID="$(cat "$WORK/new-pid")"
fi
[ ! -e "$APP.update-backup" ]
printf 'helper %s test passed\n' "$STAGED_MODE"
