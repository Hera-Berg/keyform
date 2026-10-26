#!/bin/sh
# start.sh — starts the private headless display and the reference editor
# as one systemd user service with a hard memory cap. The editor's launcher
# lets Java size its heap from all of RAM; inside the cap the JVM sizes it
# from the cgroup limit instead, and the machine stays responsive.
# Stop with: systemctl --user stop keyform-bb
set -e
CAP=${KEYFORM_BB_MEMORY:-6G}
DIR="$HOME/.wine-cubism/drive_c/Program Files/Live2D Cubism 5.3"
LOG=${XDG_RUNTIME_DIR:-/tmp}/keyform-bb-weston.log
systemd-run --user --unit=keyform-bb --collect \
  -p MemoryMax="$CAP" -p MemorySwapMax=0 \
  --working-directory="$DIR" \
  sh -c "weston --backend=headless --socket=cubism-bb --xwayland \
           --width=1920 --height=1080 --idle-time=0 --log='$LOG' &
         sleep 3
         env -u WAYLAND_DISPLAY DISPLAY=:2 WINEPREFIX='$HOME/.wine-cubism' \
           wine CubismEditor5.exe
         wait"
echo "started keyform-bb (memory cap $CAP); weston log: $LOG"
