#!/bin/sh
# display.sh start|stop — a private X display for the end-to-end test.
# Prints DISPLAY=:N on start. Uses Xvfb when installed (CI), otherwise a
# headless weston with Xwayland (local), as a memory-capped user service.
set -eu
UNIT=keyform-e2e
case "${1:-start}" in
stop)
  systemctl --user stop "$UNIT" 2>/dev/null || true
  [ -f "${XDG_RUNTIME_DIR:-/tmp}/keyform-e2e.pid" ] && kill "$(cat "${XDG_RUNTIME_DIR:-/tmp}/keyform-e2e.pid")" 2>/dev/null || true
  exit 0 ;;
esac
before=$(ls /tmp/.X11-unix 2>/dev/null | sort)
if command -v Xvfb >/dev/null 2>&1; then
  n=99
  Xvfb ":$n" -screen 0 1920x1200x24 -nolisten tcp >/dev/null 2>&1 &
  echo $! > "${XDG_RUNTIME_DIR:-/tmp}/keyform-e2e.pid"
  sleep 2
  echo "DISPLAY=:$n"
  exit 0
fi
# Local: headless weston running a rootful Xwayland (a bare X server with
# no window manager, like Xvfb), in one memory-capped user service.
n=${KEYFORM_E2E_DISPLAY:-7}
systemd-run --user --unit="$UNIT" --collect -p MemoryMax=6G -p MemorySwapMax=0 \
  sh -c "weston --backend=headless --socket=keyform-e2e --width=1920 --height=1200 --idle-time=0 &
         sleep 2
         WAYLAND_DISPLAY=keyform-e2e Xwayland :$n -geometry 1920x1200 -nolisten tcp
         wait" >/dev/null
for i in $(seq 1 40); do
  sleep 0.5
  if [ -S "/tmp/.X11-unix/X$n" ]; then echo "DISPLAY=:$n"; exit 0; fi
done
echo "no X display appeared" >&2
exit 1
