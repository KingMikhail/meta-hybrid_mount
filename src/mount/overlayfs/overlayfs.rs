use std::{
    ffi::CString,
    fs::create_dir,
    os::fd::AsFd,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use procfs::process::Process;
use rustix::{
    fs::CWD,
    mount::{
        FsMountFlags, FsOpenFlags, MountAttrFlags, MountFlags, MountPropagationFlags,
        MoveMountFlags, OpenTreeFlags, fsconfig_create, fsconfig_set_string, fsmount, fsopen,
        mount, mount_change, move_mount, open_tree,
    },
};

use crate::{defs::OVERLAY_SOURCE, mount::overlayfs::utils::umount_dir};

pub fn mount_overlayfs(
    lower_dirs: &[String],
    lowest: &str,
    upperdir: Option<PathBuf>,
    workdir: Option<PathBuf>,
    dest: impl AsRef<Path>,
) -> Result<()> {
    let lowerdir_config = lower_dirs
        .iter()
        .map(|s| s.as_ref())
        .chain(std::iter::once(lowest))
        .collect::<Vec<_>>()
        .join(":");
    tracing::info!(
        "mount overlayfs on {:?}, lowerdir={}, upperdir={:?}, workdir={:?}",
        dest.as_ref(),
        lowerdir_config,
        upperdir,
        workdir
    );

    let upperdir = upperdir
        .filter(|up| up.exists())
        .map(|e| e.display().to_string());
    let workdir = workdir
        .filter(|wd| wd.exists())
        .map(|e| e.display().to_string());

    let result = (|| {
        let fs = fsopen("overlay", FsOpenFlags::FSOPEN_CLOEXEC)?;
        let fs = fs.as_fd();
        fsconfig_set_string(fs, "lowerdir", &lowerdir_config)?;
        if let (Some(upperdir), Some(workdir)) = (&upperdir, &workdir) {
            fsconfig_set_string(fs, "upperdir", upperdir)?;
            fsconfig_set_string(fs, "workdir", workdir)?;
        }
        fsconfig_set_string(fs, "source", OVERLAY_SOURCE)?;
        fsconfig_create(fs)?;
        let mount = fsmount(fs, FsMountFlags::FSMOUNT_CLOEXEC, MountAttrFlags::empty())?;
        move_mount(
            mount.as_fd(),
            "",
            CWD,
            dest.as_ref(),
            MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
        )
    })();

    if let Err(e) = result {
        tracing::warn!("fsopen mount failed: {:#}, fallback to mount", e);
        let mut data = format!("lowerdir={lowerdir_config}");
        if let (Some(upperdir), Some(workdir)) = (upperdir, workdir) {
            data = format!("{data},upperdir={upperdir},workdir={workdir}");
        }
        mount(
            OVERLAY_SOURCE,
            dest.as_ref(),
            "overlay",
            MountFlags::empty(),
            Some(CString::new(data)?.as_c_str()),
        )?;
    }
    Ok(())
}

pub fn mount_devpts(dest: impl AsRef<Path>) -> Result<()> {
    create_dir(dest.as_ref())?;
    mount(
        OVERLAY_SOURCE,
        dest.as_ref(),
        "devpts",
        MountFlags::empty(),
        Some(CString::new("newinstance")?.as_c_str()),
    )?;
    mount_change(dest.as_ref(), MountPropagationFlags::PRIVATE).context("make devpts private")?;
    Ok(())
}

pub fn mount_tmpfs(dest: impl AsRef<Path>) -> Result<()> {
    tracing::info!("mount tmpfs on {}", dest.as_ref().display());
    match fsopen("tmpfs", FsOpenFlags::FSOPEN_CLOEXEC) {
        Result::Ok(fs) => {
            let fs = fs.as_fd();
            fsconfig_set_string(fs, "source", OVERLAY_SOURCE)?;
            fsconfig_create(fs)?;
            let mount = fsmount(fs, FsMountFlags::FSMOUNT_CLOEXEC, MountAttrFlags::empty())?;
            move_mount(
                mount.as_fd(),
                "",
                CWD,
                dest.as_ref(),
                MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
            )?;
        }
        _ => {
            use rustix::mount::{MountFlags, mount};

            use crate::defs::OVERLAY_SOURCE;

            mount(
                OVERLAY_SOURCE,
                dest.as_ref(),
                "tmpfs",
                MountFlags::empty(),
                None,
            )?;
        }
    }
    mount_change(dest.as_ref(), MountPropagationFlags::PRIVATE).context("make tmpfs private")?;
    let pts_dir = format!("{}/pts", dest.as_ref().display());
    if let Err(e) = mount_devpts(pts_dir) {
        tracing::warn!("do devpts mount failed: {}", e);
    }
    Ok(())
}

pub fn bind_mount(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    tracing::info!(
        "bind mount {} -> {}",
        from.as_ref().display(),
        to.as_ref().display()
    );
    match open_tree(
        CWD,
        from.as_ref(),
        OpenTreeFlags::OPEN_TREE_CLOEXEC
            | OpenTreeFlags::OPEN_TREE_CLONE
            | OpenTreeFlags::AT_RECURSIVE,
    ) {
        Result::Ok(tree) => {
            move_mount(
                tree.as_fd(),
                "",
                CWD,
                to.as_ref(),
                MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,
            )?;
        }
        _ => {
            mount(
                from.as_ref(),
                to.as_ref(),
                "",
                MountFlags::BIND | MountFlags::REC,
                None,
            )?;
        }
    }
    Ok(())
}

fn mount_overlay_child(
    mount_point: &str,
    relative: &String,
    module_roots: &Vec<String>,
    stock_root: &String,
) -> Result<()> {
    if !module_roots
        .iter()
        .any(|lower| Path::new(&format!("{lower}{relative}")).exists())
    {
        return bind_mount(stock_root, mount_point);
    }
    if !Path::new(&stock_root).is_dir() {
        return Ok(());
    }
    let mut lower_dirs: Vec<String> = vec![];
    for lower in module_roots {
        let lower_dir = format!("{lower}{relative}");
        let path = Path::new(&lower_dir);
        if path.is_dir() {
            lower_dirs.push(lower_dir);
        } else if path.exists() {
            // stock root has been blocked by this file
            return Ok(());
        }
    }
    if lower_dirs.is_empty() {
        return Ok(());
    }
    // merge modules and stock
    if let Err(e) = mount_overlayfs(&lower_dirs, stock_root, None, None, mount_point) {
        tracing::warn!("failed: {:#}, fallback to bind mount", e);
        bind_mount(stock_root, mount_point)?;
    }
    Ok(())
}

pub fn mount_overlay(
    root: &String,
    module_roots: &Vec<String>,
    workdir: Option<PathBuf>,
    upperdir: Option<PathBuf>,
) -> Result<()> {
    tracing::info!("mount overlay for {}", root);
    std::env::set_current_dir(root).with_context(|| format!("failed to chdir to {root}"))?;
    let stock_root = ".";

    // collect child mounts before mounting the root
    let mounts = Process::myself()?
        .mountinfo()
        .with_context(|| "get mountinfo")?;
    let mut mount_seq = mounts
        .0
        .iter()
        .filter(|m| {
            m.mount_point.starts_with(root) && !Path::new(&root).starts_with(&m.mount_point)
        })
        .map(|m| m.mount_point.to_str())
        .collect::<Vec<_>>();
    mount_seq.sort();
    mount_seq.dedup();

    mount_overlayfs(module_roots, root, upperdir, workdir, root)
        .with_context(|| "mount overlayfs for root failed")?;
    for mount_point in mount_seq.iter() {
        let Some(mount_point) = mount_point else {
            continue;
        };
        let relative = mount_point.replacen(root, "", 1);
        let stock_root: String = format!("{stock_root}{relative}");
        if !Path::new(&stock_root).exists() {
            continue;
        }
        if let Err(e) = mount_overlay_child(mount_point, &relative, module_roots, &stock_root) {
            tracing::warn!(
                "failed to mount overlay for child {}: {:#}, revert",
                mount_point,
                e
            );
            umount_dir(root).with_context(|| format!("failed to revert {root}"))?;
            bail!(e);
        }
    }
    Ok(())
}
