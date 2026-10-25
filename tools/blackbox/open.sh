#!/bin/sh
# open.sh WINPATH — File > Open a project (dialog must not be open yet).
set -e
B="python3 -I $(dirname "$0")/bb.py"
$B key ctrl+o
$B wait "Open" 10
$B filename "Open"; $B key ctrl+a; $B type "$1"; $B key Return
$B waitgone "Open" 10
