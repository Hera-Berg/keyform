#!/bin/sh
# export.sh MAG WINPATH — File > Export Image/Video > Image at MAG %, saved to WINPATH.
set -e
B="python3 -I $(dirname "$0")/bb.py"
$B click 21 65; sleep 1
$B move 100 294; sleep 0.5
$B move 330 294; sleep 0.3
$B click 335 294
$B wait "PNG Image Output Settings" 10
$B click 218 103; $B key ctrl+a; $B type "$1"; $B key Tab; sleep 0.5
$B click 180 334
$B wait "Save" 10
$B filename "Save"; $B key ctrl+a; $B type "$2"; $B key Return
$B waitgone "Save" 10
# The editor opens the output folder in Explorer afterwards: close it.
DIR=$(echo "$2" | sed 's/.*\\\(.*\)\\[^\\]*$/\1/')
$B wait "$DIR" 20 && $B close "$DIR" && $B waitgone "$DIR" 10
