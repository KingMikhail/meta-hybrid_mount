use std::{
    fs::{create_dir_all, remove_dir_all},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result, bail};
#[cfg(any(target_os = "linux", target_os = "android"))]
use extattr::{Flags as XattrFlags, lsetxattr};
use regex_lite::Regex;

use crate::defs::{self, TMPFS_CANDIDATES};

const SELINUX_XATTR: &str = "security.selinux";

static MODULE_ID_REGEX: OnceLock<Regex> = OnceLock::new();

pub fn validate_module_id(module_id: &str) -> Result<()> {
    let re = MODULE_ID_REGEX
        .get_or_init(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9._-]+$").expect("Invalid Regex pattern"));
    if re.is_match(module_id) {
        Ok(())
    } else {
        bail!("Invalid module ID: '{module_id}'. Must match /^[a-zA-Z][a-zA-Z0-9._-]+$/")
    }
}

pub fn check_zygisksu_enforce_status() -> bool {
    std::fs::read_to_string("/data/adb/zygisksu/denylist_enforce")
        .map(|s| s.trim() != "0")
        .unwrap_or(false)
}

pub fn lsetfilecon<P: AsRef<Path>>(path: P, con: &str) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        if let Err(e) = lsetxattr(
            path.as_ref(),
            SELINUX_XATTR,
            con.as_bytes(),
            XattrFlags::empty(),
        ) {
            let io_err = std::io::Error::from(e);
            log::debug!(
                "lsetfilecon: {} -> {} failed: {}",
                path.as_ref().display(),
                con,
                io_err
            );
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = path;
        let _ = con;
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn lgetfilecon<P: AsRef<Path>>(path: P) -> Result<String> {
    let con = extattr::lgetxattr(path.as_ref(), SELINUX_XATTR).with_context(|| {
        format!(
            "Failed to get SELinux context for {}",
            path.as_ref().display()
        )
    })?;
    let con_str = String::from_utf8_lossy(&con).trim_matches('\0').to_string();

    Ok(con_str)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn lgetfilecon<P: AsRef<Path>>(_path: P) -> Result<String> {
    Ok("u:object_r:system_file:s0".to_string())
}

pub fn ensure_dir_exists<T: AsRef<Path>>(dir: T) -> Result<()> {
    if !dir.as_ref().exists() {
        create_dir_all(&dir)?;
    }
    Ok(())
}

fn is_ok_empty<P: AsRef<Path>>(dir: P) -> bool {
    if !dir.as_ref().exists() {
        return false;
    }
    dir.as_ref()
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_none())
}

pub fn select_temp_dir() -> Result<PathBuf> {
    for path_str in TMPFS_CANDIDATES {
        let path = Path::new(path_str);
        if is_ok_empty(path) {
            log::info!("Selected dynamic temp root: {}", path.display());
            return Ok(path.to_path_buf());
        }
    }
    let run_dir = Path::new(defs::RUN_DIR);
    ensure_dir_exists(run_dir)?;
    let work_dir = run_dir.join("workdir");
    Ok(work_dir)
}

// Helper specific to Magic Mount
pub fn mount_tmpfs<P: AsRef<Path>>(target: P, source: &str) -> Result<()> {
    use rustix::mount::{MountFlags, mount};
    mount(
        source,
        target.as_ref(),
        "tmpfs",
        MountFlags::empty(),
        None::<&str>,
    )
    .with_context(|| format!("Failed to mount tmpfs at {}", target.as_ref().display()))
}

pub fn mount_image<P: AsRef<Path>, Q: AsRef<Path>>(img: P, target: Q) -> Result<()> {
    use rustix::mount::{MountFlags, mount};
    mount(
        img.as_ref(),
        target.as_ref(),
        "ext4",
        MountFlags::empty(),
        None::<&str>,
    )
    .context("Failed to mount ext4 image")
}

pub fn repair_image<P: AsRef<Path>>(_img: P) -> Result<()> {
    Ok(())
}

pub fn is_mounted<P: AsRef<Path>>(path: P) -> bool {
    if let Ok(mounts) = procfs::process::Process::myself().and_then(|p| p.mountinfo()) {
        return mounts
            .0
            .iter()
            .any(|m| m.mount_point == path.as_ref().to_path_buf());
    }
    false
}

pub fn is_erofs_supported() -> bool {
    if let Ok(filesystems) = std::fs::read_to_string("/proc/filesystems") {
        return filesystems.contains("erofs");
    }
    false
}

pub fn create_erofs_image<P: AsRef<Path>, Q: AsRef<Path>>(_src: P, _dst: Q) -> Result<()> {
    // Placeholder: Need external tool or library to create erofs
    // For now assuming the tool exists or logic is handled elsewhere
    // In old version this was calling mkfs.erofs binary
    use std::process::Command;
    let status = Command::new("/data/adb/meta-hybrid/tools/mkfs.erofs")
        .arg("-zLZ4HC")
        .arg(_dst.as_ref())
        .arg(_src.as_ref())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("mkfs.erofs failed")
    }
}

pub fn mount_erofs_image<P: AsRef<Path>, Q: AsRef<Path>>(img: P, target: Q) -> Result<()> {
    use rustix::mount::{MountFlags, mount};
    mount(
        img.as_ref(),
        target.as_ref(),
        "erofs",
        MountFlags::RDONLY,
        None::<&str>,
    )
    .context("Failed to mount erofs image")
}

pub fn is_overlay_xattr_supported<P: AsRef<Path>>(_path: P) -> Result<()> {
    // Simple check stub
    Ok(())
}
