use std::os::windows::ffi::OsStrExt;
use std::path::Path;

pub fn move_new_file(source: &Path, target: &Path) -> std::io::Result<()> {
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileW(source: *const u16, target: *const u16) -> i32;
    }

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: Both UTF-16 paths are terminated and live through the Win32 call.
    if unsafe { MoveFileW(source.as_ptr(), target.as_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
