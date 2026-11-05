#!/bin/sh
# free.sh — after start.sh: choose "Start as FREE version", confirm the
# welcome message and close the Home window.
B="python3 -I $(dirname "$0")/bb.py"
$B wait "Start" 120
XY=$($B windows | grep "'Start'" | sed 's/.*+\([0-9-]*\)+\([0-9-]*\)$/\1 \2/')
X=${XY% *}; Y=${XY#* }
# The fourth button of the Start dialog.
until ! $B windows | grep -q "'Start'"; do
  $B click $((X + 192)) $((Y + 257)); sleep 2
done
$B wait "Confirm" 30 && $B key Return && $B waitgone "Confirm" 10
$B wait "Home" 30 && $B close "Home"
