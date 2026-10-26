//! The Windows security calls, the only unsafe code in this crate
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION, AclSizeInformation,
    AddAccessAllowedAceEx, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
    GetAclInformation, GetLengthSid, GetTokenInformation, InitializeAcl, OBJECT_INHERIT_ACE,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

// FILE_ALL_ACCESS, which windows-sys names only under a feature this crate
// has no other use for
const FILE_ALL_ACCESS: u32 = 0x001f_01ff;

// The `AceType` of an ACE that grants and of one that refuses, which
// windows-sys does not name
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;

pub(super) fn create_dir(path: &Path) -> io::Result<()> {
    fs::create_dir(path)?;
    restrict(path, true).inspect_err(|_| {
        let _ = fs::remove_dir(path);
    })
}

// Made empty and narrowed before the caller writes, so nothing secret is
// ever in a file others can read.
pub(super) fn create_file(path: &Path) -> io::Result<fs::File> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(e) = restrict(path, false) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(e);
    }
    Ok(file)
}

pub(super) fn restrict_dir(path: &Path) -> io::Result<()> {
    restrict(path, true)
}

pub(super) fn is_owner(path: &Path) -> io::Result<bool> {
    let user = User::current()?;
    Ok(Security::of(path)?.owned_by(&user))
}

pub(super) fn is_owner_only(path: &Path) -> io::Result<bool> {
    let user = User::current()?;
    let security = Security::of(path)?;
    Ok(security.owned_by(&user) && security.grants_only(&user)?)
}

// Makes `path` the current user's, with a protected DACL of one ACE that
// gives the user everything, and a folder's ACE passed to what it will hold.
// An owner that is already the user is left, which needs no right to change it.
fn restrict(path: &Path, folder: bool) -> io::Result<()> {
    let user = User::current()?;
    let owned = Security::of(path)?.owned_by(&user);
    let acl = Acl::granting(&user, folder)?;
    let name = wide(path);
    let mut parts = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    let mut owner = ptr::null_mut();
    if !owned {
        parts |= OWNER_SECURITY_INFORMATION;
        owner = user.sid();
    }
    // SAFETY: `name` is NUL-terminated and outlives the call. `owner` is null
    // or the SID in `user`, and `acl` is an initialized ACL, both live for the call.
    win32(unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            parts,
            owner,
            ptr::null_mut(),
            acl.as_ptr(),
            ptr::null(),
        )
    })
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(once(0)).collect()
}

fn win32(code: u32) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// The current user's SID, held in the buffer `GetTokenInformation` filled. It
// is of `u64`s so the `TOKEN_USER` in it is aligned.
struct User(Vec<u64>);

impl User {
    fn current() -> io::Result<Self> {
        let mut token: HANDLE = ptr::null_mut();
        // SAFETY: `GetCurrentProcess` is a pseudo handle that needs no closing,
        // and `token` is written only when the call succeeds.
        check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
        // SAFETY: the call above made this handle and nothing else owns it.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut needed = 0;
        // SAFETY: `token` is live, and a null buffer of length 0 asks for the
        // size, which the call writes to `needed`.
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
        // SAFETY: `token` is live and `buffer` holds at least `needed` bytes.
        check(unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        })?;
        Ok(Self(buffer))
    }

    // Points into `self`, so it holds only while `self` does.
    fn sid(&self) -> PSID {
        // SAFETY: the buffer holds a `TOKEN_USER` that `GetTokenInformation`
        // filled in, aligned as the buffer's `u64`s are, and its `Sid` points
        // into the same buffer.
        unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
}

// An ACL of one allowed ACE, in a buffer of `u64`s so it is aligned.
struct Acl(Vec<u64>);

impl Acl {
    fn granting(user: &User, folder: bool) -> io::Result<Self> {
        // SAFETY: `user.sid()` is the valid SID in `user`.
        let sid_len = unsafe { GetLengthSid(user.sid()) } as usize;
        // An ACE's `SidStart` is the first word of the SID, so it is counted once.
        let bytes = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid_len;
        let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>())];
        let length = u32::try_from(buffer.len() * size_of::<u64>())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let acl = buffer.as_mut_ptr().cast::<ACL>();
        let inherit = if folder {
            OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
        } else {
            0
        };
        // SAFETY: `acl` points at `length` writable bytes, and `user.sid()` is
        // the valid SID in `user`, which the ACE is copied from.
        unsafe {
            check(InitializeAcl(acl, length, ACL_REVISION))?;
            check(AddAccessAllowedAceEx(
                acl,
                ACL_REVISION,
                inherit,
                FILE_ALL_ACCESS,
                user.sid(),
            ))?;
        }
        Ok(Self(buffer))
    }

    fn as_ptr(&self) -> *const ACL {
        self.0.as_ptr().cast()
    }
}

// The owner and DACL of a path, in the descriptor Windows allocated for them.
struct Security {
    descriptor: *mut c_void,
    owner: PSID,
    dacl: *mut ACL,
}

impl Security {
    fn of(path: &Path) -> io::Result<Self> {
        let name = wide(path);
        let mut security = Self {
            descriptor: ptr::null_mut(),
            owner: ptr::null_mut(),
            dacl: ptr::null_mut(),
        };
        // SAFETY: `name` is NUL-terminated and outlives the call. Each pointer
        // out is a live field, and on success `descriptor` holds what `owner`
        // and `dacl` point into, until `drop` frees it.
        win32(unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut security.owner,
                ptr::null_mut(),
                &mut security.dacl,
                ptr::null_mut(),
                &mut security.descriptor,
            )
        })?;
        Ok(security)
    }

    fn owned_by(&self, user: &User) -> bool {
        // SAFETY: both are valid SIDs, one in `self`'s descriptor and one in `user`.
        !self.owner.is_null() && unsafe { EqualSid(self.owner, user.sid()) } != 0
    }

    // No DACL is full access for everyone. Every ACE must be the user's, or
    // refuse someone, and any other kind of ACE is taken to grant.
    fn grants_only(&self, user: &User) -> io::Result<bool> {
        if self.dacl.is_null() {
            return Ok(false);
        }
        let mut info = ACL_SIZE_INFORMATION {
            AceCount: 0,
            AclBytesInUse: 0,
            AclBytesFree: 0,
        };
        // SAFETY: `dacl` is a valid ACL in `self`'s descriptor, and `info` is
        // the structure the size class fills, of the size passed.
        check(unsafe {
            GetAclInformation(
                self.dacl,
                ptr::from_mut(&mut info).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        })?;
        for index in 0..info.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            // SAFETY: `index` is below the ACL's ACE count, and `ace` is
            // written on success with a pointer into the ACL.
            check(unsafe { GetAce(self.dacl, index, &mut ace) })?;
            // SAFETY: every ACE starts with an `ACE_HEADER`, and an allowed ACE
            // is an `ACCESS_ALLOWED_ACE` whose SID starts at `SidStart`.
            let granted_to = unsafe {
                match (*ace.cast::<ACE_HEADER>()).AceType {
                    ACCESS_DENIED_ACE_TYPE => continue,
                    ACCESS_ALLOWED_ACE_TYPE => {
                        ptr::addr_of!((*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart)
                            .cast_mut()
                            .cast::<c_void>()
                    }
                    _ => return Ok(false),
                }
            };
            // SAFETY: both are valid SIDs, one in the ACL and one in `user`.
            if unsafe { EqualSid(granted_to, user.sid()) } == 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            // SAFETY: `GetNamedSecurityInfoW` allocated the descriptor for the
            // caller to free with `LocalFree`, and nothing uses it after this.
            unsafe { LocalFree(self.descriptor) };
        }
    }
}
