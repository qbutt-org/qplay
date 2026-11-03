use std::path::PathBuf;

pub fn executable(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(target_os = "linux")]
pub fn xdg_directory(variable: &str, default: &str) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(path);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("HOME needs an absolute directory.")?;
    Ok(home.join(default))
}

pub fn cache_directory(data: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    if xdg_directory("XDG_DATA_HOME", ".local/share").is_ok_and(|root| root.join("qplay") == data) {
        if let Ok(root) = xdg_directory("XDG_CACHE_HOME", ".cache") {
            return root.join("qplay");
        }
    }
    data.to_path_buf()
}
