//! Helpers shared by the app's tests.

use std::path::Path;

/// Makes `to` refer to `from`: a symbolic link on Unix, a copy elsewhere
/// (Windows needs a privilege for symbolic links). Errors are ignored: the
/// tests that use it open `to` next and fail there.
pub fn link(from: &Path, to: &Path) {
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(from, to);
    #[cfg(not(unix))]
    let _ = std::fs::copy(from, to);
}
