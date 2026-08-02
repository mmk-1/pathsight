use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub inode: u64,
    pub dev_major: u32,
    pub dev_minor: u32,
    pub mount_id: u64,
}

pub fn resolve_path(
    root: &impl AsRawFd,
    cwd: &impl AsRawFd,
    path: &Path,
) -> Result<ResolvedPath, String> {
    if path.is_absolute() {
        open_how(
            root,
            path,
            libc::RESOLVE_IN_ROOT | libc::RESOLVE_NO_MAGICLINKS,
        )
    } else {
        open_how(cwd, path, libc::RESOLVE_NO_MAGICLINKS)
    }
}

fn open_how(dir: &impl AsRawFd, path: &Path, resolve: u64) -> Result<ResolvedPath, String> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| {
        format!("path contains interior null byte: {}", path.display())
    })?;

    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_PATH | libc::O_CLOEXEC) as u64;
    how.resolve = resolve;

    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            dir.as_raw_fd(),
            c_path.as_ptr(),
            &how as *const libc::open_how,
            std::mem::size_of::<libc::open_how>(),
        )
    };
    if fd < 0 {
        return Err(map_open_error(path, std::io::Error::last_os_error()));
    }

    let owned = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    statx_path_fd(&owned).map_err(|e| format!("cannot stat {}: {e}", path.display()))
}

fn statx_path_fd(fd: &OwnedFd) -> Result<ResolvedPath, String> {
    let mut buf: libc::statx = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::statx(
            fd.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_INO | libc::STATX_MNT_ID,
            &mut buf,
        )
    };
    if rc < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if buf.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err("kernel did not return mount id (need Linux 5.8+)".into());
    }

    Ok(ResolvedPath {
        inode: buf.stx_ino,
        dev_major: buf.stx_dev_major,
        dev_minor: buf.stx_dev_minor,
        mount_id: buf.stx_mnt_id,
    })
}

fn map_open_error(path: &Path, err: std::io::Error) -> String {
    let display = path.display();
    match err.raw_os_error() {
        Some(libc::ENOENT) => format!("no such path {display}"),
        Some(libc::EACCES) | Some(libc::EPERM) => {
            format!("permission denied opening {display}")
        }
        Some(libc::ENOTDIR) => format!("not a directory in path {display}"),
        Some(libc::ENOSYS) | Some(libc::EOPNOTSUPP) => {
            "openat2 is not supported on this kernel (need Linux 5.6+)".into()
        }
        Some(libc::ELOOP) => format!("too many symlinks resolving {display}"),
        _ => format!("cannot open {display}: {err}"),
    }
}
