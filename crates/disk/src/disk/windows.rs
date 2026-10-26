//! The Windows disk calls, the only unsafe code in this crate
#![allow(unsafe_code)]

use std::fs::{Metadata, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_DELETE_PENDING};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_STANDARD_INFO, FileStandardInfo, GetDiskFreeSpaceExW,
    GetFileInformationByHandle, GetFileInformationByHandleEx,
};

use super::Allocation;

pub(super) fn free_bytes(on: &Path) -> io::Result<u64> {
    let mut name: Vec<u16> = on.as_os_str().encode_wide().collect();
    if name.contains(&0) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    name.push(0);
    let mut available = 0;
    // SAFETY: `name` is NUL-terminated and `available` is writable for the call.
    // Null pointers omit the two totals the caller does not need.
    check(unsafe {
        GetDiskFreeSpaceExW(
            name.as_ptr(),
            &mut available,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    })?;
    Ok(available)
}

pub(super) fn allocation(path: &Path, _meta: &Metadata) -> io::Result<Option<Allocation>> {
    // Attribute access permits reading files without permission to read their contents.
    // Reparse points count themselves, including links to folders.
    let file = match OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
    {
        Ok(file) => file,
        // Windows can report a delete-pending file as access denied.
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
                || error.raw_os_error() == Some(ERROR_DELETE_PENDING as i32) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut identity = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` is live and `identity` is the structure the call writes.
    check(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut identity) })?;
    let mut standard = FILE_STANDARD_INFO::default();
    // SAFETY: `file` is live and `standard` is the structure FileStandardInfo
    // writes, with its exact size passed as the buffer length.
    check(unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            ptr::from_mut(&mut standard).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    })?;
    let bytes = u64::try_from(standard.AllocationSize)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    let index = (u64::from(identity.nFileIndexHigh) << 32) | u64::from(identity.nFileIndexLow);
    Ok(Some(Allocation {
        bytes,
        linked: (identity.nNumberOfLinks > 1)
            .then(|| (u64::from(identity.dwVolumeSerialNumber), index)),
    }))
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
