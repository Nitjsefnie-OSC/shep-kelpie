use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub(super) fn create_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

pub(super) fn create_file(path: &Path) -> io::Result<fs::File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

pub(super) fn restrict_dir(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

pub(super) fn is_owner(path: &Path) -> io::Result<bool> {
    Ok(fs::metadata(path)?.uid() == rustix::process::getuid().as_raw())
}

pub(super) fn is_owner_only(path: &Path) -> io::Result<bool> {
    let meta = fs::metadata(path)?;
    Ok(meta.uid() == rustix::process::getuid().as_raw() && meta.mode() & 0o077 == 0)
}
