use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

pub(crate) struct ConfigFileLock {
    file: File,
}

impl ConfigFileLock {
    pub(crate) fn acquire(config_path: &Path) -> Result<Self> {
        let parent = config_parent_dir(config_path);
        ensure_dir_secure(parent)?;
        let lock_path = lock_file_path(config_path);
        reject_symlink(&lock_path)?;

        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
            if let Some(flag) = unix_o_nofollow() {
                options.custom_flags(flag);
            }
        }
        let file = options
            .open(&lock_path)
            .with_context(|| format!("failed to open CLI config lock {}", lock_path.display()))?;
        verify_open_file_security(&file, &lock_path)?;
        platform::lock(&file)
            .with_context(|| format!("failed to lock CLI config lock {}", lock_path.display()))?;
        Ok(Self { file })
    }
}

impl Drop for ConfigFileLock {
    fn drop(&mut self) {
        let _ = platform::unlock(&self.file);
    }
}

pub(crate) fn config_parent_dir(config_path: &Path) -> &Path {
    config_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub(crate) fn lock_file_path(config_path: &Path) -> PathBuf {
    let mut name = config_path
        .file_name()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "cli.json".into());
    name.push(".lock");
    config_parent_dir(config_path).join(name)
}

pub(crate) fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                bail!("refusing to use symlink at {}", path.display());
            }
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(anyhow::Error::new(err)
            .context(format!("failed to inspect metadata for {}", path.display()))),
    }
}

pub(crate) fn ensure_dir_secure(dir: &Path) -> Result<()> {
    reject_symlink(dir)?;
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create CLI config directory {}", dir.display()))?;
    reject_symlink(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = fs::symlink_metadata(dir)
            .with_context(|| format!("failed to inspect CLI config directory {}", dir.display()))?;
        if meta.uid() != current_euid() {
            bail!(
                "CLI config directory {} is not owned by the current user",
                dir.display()
            );
        }
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).with_context(|| {
            format!(
                "failed to set 0700 permissions on CLI config directory {}",
                dir.display()
            )
        })?;
    }
    Ok(())
}

pub(crate) fn verify_open_file_security(file: &File, path: &Path) -> Result<()> {
    let meta = file
        .metadata()
        .with_context(|| format!("failed to inspect open file {}", path.display()))?;
    if !meta.is_file() {
        bail!("CLI config path {} is not a regular file", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != current_euid() {
            bail!(
                "CLI config file {} is not owned by the current user",
                path.display()
            );
        }
        if meta.mode() & 0o077 != 0 {
            bail!(
                "CLI config file {} has insecure permissions {:04o}; run `chmod 600 {}`",
                path.display(),
                meta.mode() & 0o777,
                path.display()
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn unix_o_nofollow() -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        Some(0x0100)
    }
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
    {
        Some(0o100_000)
    }
    #[cfg(all(
        target_os = "linux",
        not(any(target_arch = "aarch64", target_arch = "arm"))
    ))]
    {
        Some(0o400_000)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(unix)]
pub(crate) fn current_euid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

#[cfg(unix)]
mod platform {
    use std::{fs::File, io, os::fd::AsRawFd};

    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }

    const LOCK_EX: i32 = 2;
    const LOCK_UN: i32 = 8;

    pub(super) fn lock(file: &File) -> io::Result<()> {
        call(file, LOCK_EX)
    }

    pub(super) fn unlock(file: &File) -> io::Result<()> {
        call(file, LOCK_UN)
    }

    fn call(file: &File, operation: i32) -> io::Result<()> {
        if unsafe { flock(file.as_raw_fd(), operation) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::{ffi::c_void, fs::File, io, os::windows::io::AsRawHandle};

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut c_void,
    }

    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LockFileEx(
            file: *mut c_void,
            flags: u32,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(
            file: *mut c_void,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }

    pub(super) fn lock(file: &File) -> io::Result<()> {
        let mut overlapped = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: std::ptr::null_mut(),
        };
        let ok = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK,
                0,
                1,
                0,
                &mut overlapped,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn unlock(file: &File) -> io::Result<()> {
        let mut overlapped = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            event: std::ptr::null_mut(),
        };
        let ok = unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
