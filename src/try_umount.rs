use std::{
    fs::read_dir,
    path::Path,
    sync::{
        LazyLock, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
use ksu::TryUmount;

use crate::defs::{DISABLE_FILE_NAME, REMOVE_FILE_NAME, SKIP_MOUNT_FILE_NAME};

static LAST: AtomicBool = AtomicBool::new(false);
pub static TMPFS: OnceLock<String> = OnceLock::new();
pub static LIST: LazyLock<Mutex<TryUmount>> = LazyLock::new(|| Mutex::new(TryUmount::new()));

pub fn send_unmountable<P>(target: P) -> Result<()>
where
    P: AsRef<Path>,
{
    if LAST.load(Ordering::Relaxed) {
        return Ok(());
    }

    // Check if we are in a ZygiskSU Enforce environment AND using debug_ramdisk
    // If so, we might need to be careful about unmounting to avoid detection triggers or conflicts
    if crate::utils::check_zygisksu_enforce_status()
        && TMPFS.get().is_some_and(|s| s.trim() == "/debug_ramdisk")
    {
        log::warn!(
            "ZygiskSU/ZN Enforce detected with debug_ramdisk. Canceling automatic try_umount to prevent conflicts."
        );
        LAST.store(true, Ordering::Relaxed);
        return Ok(());
    }

    // Only scan modules if we haven't decided to stop yet
    if !LAST.load(Ordering::Relaxed) {
        if let Ok(entries) = read_dir("/data/adb/modules") {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if !path.join("module.prop").exists() {
                    continue;
                }

                let disabled = path.join(DISABLE_FILE_NAME).exists()
                    || path.join(REMOVE_FILE_NAME).exists()
                    || path.join(SKIP_MOUNT_FILE_NAME).exists();

                if disabled {
                    continue;
                }

                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |s| s.ends_with("zygisksu"))
                {
                    if crate::utils::check_zygisksu_enforce_status()
                        && TMPFS.get().is_some_and(|s| s.trim() == "/debug_ramdisk")
                    {
                        log::warn!(
                            "ZygiskSU module detected in Enforce mode. Canceling try_umount."
                        );
                        LAST.store(true, Ordering::Relaxed);
                        return Ok(());
                    }
                }
            }
        }
    }

    LIST.lock().unwrap().add(target);

    Ok(())
}

pub fn commit() -> Result<()> {
    let mut list = LIST.lock().unwrap();
    list.flags(2);
    list.umount()?;
    Ok(())
}
