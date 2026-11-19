#!/bin/sh
# vendor.sh [--tarball OUT.tar.xz] — copies every dependency in Cargo.lock
# into vendor/ and writes .cargo/vendor.toml, so that
#
#   cargo build --release --locked --offline --config .cargo/vendor.toml
#
# builds without network access. With --tarball it also packs the sources,
# vendor/ and the config into one archive for offline release builds.
# Fetching uses the local cargo cache when it is complete (CARGO_NET_OFFLINE
# is tried first); otherwise crates.io once.
set -eu
cd "$(dirname "$0")/.."
mkdir -p .cargo
if ! CARGO_NET_OFFLINE=true cargo vendor --locked vendor > .cargo/vendor.toml 2>/dev/null; then
  cargo vendor --locked vendor > .cargo/vendor.toml
fi
echo "vendored $(ls vendor | wc -l) crates; build offline with:"
echo "  cargo build --release --locked --offline --config .cargo/vendor.toml"
if [ "${1:-}" = "--tarball" ]; then
  OUT=$2
  VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
  git ls-files -z | tar --null -T - -cf "$OUT.tmp.tar" --transform "s,^,keyform-$VERSION/,"
  tar -rf "$OUT.tmp.tar" --transform "s,^,keyform-$VERSION/," vendor .cargo/vendor.toml
  xz -T0 -c "$OUT.tmp.tar" > "$OUT"
  rm -f "$OUT.tmp.tar"
  echo "wrote $OUT"
fi
