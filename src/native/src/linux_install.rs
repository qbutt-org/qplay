use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zip::ZipArchive;

use crate::platform::xdg_directory;
use crate::runtime_info::{VERSION, verify_bundle, verify_bundle_version};

#[derive(Deserialize)]
struct Manifest {
    version: String,
    files: BTreeMap<String, String>,
}

fn destination() -> Result<PathBuf, String> {
    Ok(xdg_directory("XDG_DATA_HOME", ".local/share")?.join("qplay/app"))
}

pub fn other_instances_running(directory: &Path) -> Result<bool, String> {
    let expected = fs::metadata(directory.join("qplay"))
        .map_err(|error| format!("Inspect installed qplay: {error}"))?;
    let entries =
        fs::read_dir("/proc").map_err(|error| format!("Inspect qplay instances: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("Inspect qplay process: {error}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        let metadata = match fs::metadata(entry.path().join("exe")) {
            Ok(metadata) => metadata,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) =>
            {
                continue;
            }
            Err(error) => return Err(format!("Inspect qplay process executable: {error}")),
        };
        if metadata.dev() == expected.dev() && metadata.ino() == expected.ino() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn stage_directory(target: &Path) -> Result<PathBuf, String> {
    let parent = target.parent().ok_or("Invalid installation directory.")?;
    fs::create_dir_all(parent).map_err(|error| format!("Create installation parent: {error}"))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("Inspect installation parent: {error}"))?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err("The qplay installation parent must be an owned real directory.".into());
    }
    let stage = parent.join(format!(".install-{}", std::process::id()));
    fs::create_dir(&stage).map_err(|error| format!("Create installation staging: {error}"))?;
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Protect installation staging: {error}"))?;
    Ok(stage)
}

fn commit(stage: &Path, target: &Path, version: &str) -> Result<(), String> {
    verify_bundle_version(stage, version)?;
    if let Ok(metadata) = fs::symlink_metadata(target) {
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err("The qplay application must be an owned real directory.".into());
        }
        let bytes = fs::read(target.join("build-manifest.json"))
            .map_err(|error| format!("Read installed qplay manifest: {error}"))?;
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Read installed qplay manifest: {error}"))?;
        verify_bundle_version(target, &manifest.version)?;
        verify_owned_directory(target, target, &manifest)?;
        if other_instances_running(target)? {
            return Err(
                "Another qplay window is open. Close all qplay windows before installation.".into(),
            );
        }
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let source =
            CString::new(stage.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
        let destination =
            CString::new(target.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
        // SAFETY: Both verified owned directories are on one filesystem. Exchange
        // keeps the complete old or new installation reachable at every instant.
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                destination.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        } != 0
        {
            return Err(format!(
                "Commit qplay installation: {}",
                std::io::Error::last_os_error()
            ));
        }
    } else {
        crate::files::move_new_file(stage, target)
            .map_err(|error| format!("Commit qplay installation: {error}"))?;
    }
    File::open(target.parent().ok_or("Invalid installation directory.")?)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("Flush qplay installation: {error}"))?;
    Ok(())
}

pub fn install_directory(source: &Path) -> Result<String, String> {
    verify_bundle(source)?;
    let target = destination()?;
    if fs::canonicalize(source).ok() != fs::canonicalize(&target).ok() {
        let stage = stage_directory(&target)?;
        let result = (|| {
            let bytes =
                fs::read(source.join("build-manifest.json")).map_err(|error| error.to_string())?;
            let manifest: Manifest =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            for name in manifest.files.keys() {
                let output = stage.join(name);
                fs::create_dir_all(output.parent().ok_or("Invalid bundled file.")?)
                    .map_err(|error| format!("Create installed directory: {error}"))?;
                fs::copy(source.join(name), &output)
                    .map_err(|error| format!("Copy installed {name}: {error}"))?;
            }
            fs::write(stage.join("build-manifest.json"), bytes)
                .map_err(|error| error.to_string())?;
            commit(&stage, &target, VERSION.trim())
        })();
        if stage.exists() {
            fs::remove_dir_all(&stage)
                .map_err(|error| format!("Remove installation staging: {error}"))?;
        }
        result?;
    }
    register(&target)?;
    Ok(target.join("qplay").to_string_lossy().into_owned())
}

pub fn install_archive(archive: &Path, target: &Path, version: &str) -> Result<(), String> {
    if target != destination()? {
        return Err("Automatic updates require the registered per-user qplay installation.".into());
    }
    let stage = stage_directory(target)?;
    let result = (|| {
        let archive = File::open(archive).map_err(|error| format!("Open qplay update: {error}"))?;
        let mut archive =
            ZipArchive::new(archive).map_err(|error| format!("Read qplay update: {error}"))?;
        if archive.len() > 20000 {
            return Err("The qplay update has too many files.".into());
        }
        let mut unpacked = 0u64;
        for index in 0..archive.len() {
            let mut member = archive
                .by_index(index)
                .map_err(|error| format!("Read update member: {error}"))?;
            let name = member.name();
            if name.contains('\\')
                || name.contains(':')
                || name.starts_with('/')
                || name
                    .trim_end_matches('/')
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err("The qplay update contains an unsafe path.".into());
            }
            let mode = member.unix_mode().unwrap_or(0o644);
            if mode & 0o170000 != 0 && mode & 0o170000 != 0o100000 && mode & 0o170000 != 0o040000 {
                return Err("The qplay update contains a link or special file.".into());
            }
            unpacked = unpacked
                .checked_add(member.size())
                .ok_or("Invalid qplay update size.")?;
            if unpacked > 4 * 1024 * 1024 * 1024 {
                return Err("The qplay update is too large when unpacked.".into());
            }
            let output = stage.join(member.enclosed_name().ok_or("Unsafe update path.")?);
            if member.is_dir() {
                fs::create_dir_all(&output)
                    .map_err(|error| format!("Create update directory: {error}"))?;
                continue;
            }
            fs::create_dir_all(output.parent().ok_or("Invalid update file.")?)
                .map_err(|error| format!("Create update directory: {error}"))?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|error| format!("Stage update file: {error}"))?;
            std::io::copy(&mut member, &mut file)
                .map_err(|error| format!("Unpack update file: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("Flush update file: {error}"))?;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o777))
                .map_err(|error| format!("Set update file mode: {error}"))?;
        }
        if stage.join("qplay.portable").exists() {
            fs::remove_file(stage.join("qplay.portable"))
                .map_err(|error| format!("Remove portable update marker: {error}"))?;
        }
        commit(&stage, target, version)?;
        register(target)
    })();
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|error| format!("Remove update staging: {error}"))?;
    }
    result
}

fn register(target: &Path) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("HOME needs an absolute directory.")?;
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin)
        .map_err(|error| format!("Create user executable directory: {error}"))?;
    let link = bin.join("qplay");
    let executable = target.join("qplay");
    match fs::symlink_metadata(&link) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                && fs::read_link(&link).ok().as_ref() == Some(&executable) =>
        {
            ()
        }
        Ok(_) => {
            return Err("The user qplay command is already owned by another installation.".into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            symlink(&executable, &link)
                .map_err(|error| format!("Register qplay command: {error}"))?;
        }
        Err(error) => return Err(format!("Inspect qplay command: {error}")),
    }
    let applications = xdg_directory("XDG_DATA_HOME", ".local/share")?.join("applications");
    fs::create_dir_all(&applications)
        .map_err(|error| format!("Create desktop applications directory: {error}"))?;
    let executable = desktop_path(&executable)?;
    let icon = target
        .join("qplay.svg")
        .to_string_lossy()
        .replace('\\', "\\\\");
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=qplay\nName[ru]=qplay\nExec=\"{executable}\" %U\nIcon={icon}\nTerminal=false\nCategories=AudioVideo;Player;Video;\nMimeType=video/mp4;video/x-matroska;video/webm;audio/mpeg;\nStartupWMClass=qplay\n"
    );
    let path = applications.join("qplay.desktop");
    if let Ok(existing) = fs::read_to_string(&path) {
        if existing != desktop {
            return Err("The qplay desktop entry is already owned by another installation.".into());
        }
    }
    fs::write(path, desktop).map_err(|error| format!("Register qplay desktop entry: {error}"))?;
    Ok(())
}

fn desktop_path(path: &Path) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or("The installation path needs valid UTF-8.")?;
    if value.chars().any(char::is_control) {
        return Err("The installation path contains control characters.".into());
    }
    Ok(value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%"))
}

pub fn uninstall() -> Result<String, String> {
    let target = destination()?;
    verify_bundle(&target)?;
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(target.join("build-manifest.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Read installed qplay manifest: {error}"))?;
    verify_owned_directory(&target, &target, &manifest)?;
    if other_instances_running(&target)? {
        return Err("Close all qplay windows before uninstalling.".into());
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is missing.")?;
    let link = home.join(".local/bin/qplay");
    if fs::read_link(&link).ok().as_ref() == Some(&target.join("qplay")) {
        fs::remove_file(link).map_err(|error| format!("Remove qplay command: {error}"))?;
    }
    let desktop =
        xdg_directory("XDG_DATA_HOME", ".local/share")?.join("applications/qplay.desktop");
    let expected = format!("Exec=\"{}\" %U", desktop_path(&target.join("qplay"))?);
    if fs::read_to_string(&desktop).is_ok_and(|text| text.lines().any(|line| line == expected)) {
        fs::remove_file(desktop).map_err(|error| format!("Remove qplay desktop entry: {error}"))?;
    }
    fs::remove_dir_all(&target).map_err(|error| format!("Remove qplay application: {error}"))?;
    Ok("qplay was removed. Preferences and playback data were preserved.".into())
}

fn verify_owned_directory(
    root: &Path,
    directory: &Path,
    manifest: &Manifest,
) -> Result<(), String> {
    for entry in
        fs::read_dir(directory).map_err(|error| format!("Inspect installed files: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Inspect installed file: {error}"))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("Inspect installed file type: {error}"))?;
        if kind.is_dir() {
            verify_owned_directory(root, &path, manifest)?;
        } else {
            let name = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_str()
                .ok_or("Invalid installed filename.")?;
            if !kind.is_file()
                || name != "build-manifest.json" && !manifest.files.contains_key(name)
            {
                return Err(format!(
                    "The installation contains an unowned file. Preserve or move it before continuing: {name}"
                ));
            }
        }
    }
    Ok(())
}
