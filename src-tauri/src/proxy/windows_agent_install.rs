use std::{
    env,
    path::{Path, PathBuf},
    ptr::null_mut,
    sync::LazyLock,
};
use windows_sys::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER, Storage::Packaging::Appx::GetPackagesByPackageFamily,
};
use winreg::{
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
    RegKey,
};

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

fn display_icon_path(icon: &str) -> &Path {
    let path = match icon.rsplit_once(',') {
        Some((path, index)) if index.trim().parse::<i32>().is_ok() => path,
        _ => icon,
    };
    Path::new(path.trim().trim_matches('"'))
}

fn is_installed_executable(path: &Path, executable: &str) -> bool {
    path.is_absolute()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(executable))
        && path.is_file()
}

fn version_from_display_name<'a>(display_name: &'a str, product: &str) -> Option<&'a str> {
    static VERSION: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^[vV]?[0-9]+(?:\.[0-9]+){1,3}(?:[-+][0-9A-Za-z.+-]+)?$").unwrap()
    });
    let prefix = display_name.get(..product.len())?;
    let suffix = display_name.get(product.len()..)?.strip_prefix(' ')?.trim();
    (prefix.eq_ignore_ascii_case(product) && VERSION.is_match(suffix)).then_some(suffix)
}

pub(super) fn find_nsis_installation(product: &str, executable: &str) -> (bool, Option<String>) {
    // Electron Builder puts the install directory in a private key on older versions.
    // DisplayIcon still points to the installed executable in the public uninstall entry.
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Ok(uninstall) = root.open_subkey_with_flags(UNINSTALL_KEY, KEY_READ | view) else {
                continue;
            };
            for name in uninstall.enum_keys().flatten() {
                let Ok(entry) = uninstall.open_subkey_with_flags(name, KEY_READ | view) else {
                    continue;
                };
                let display_name: String = entry.get_value("DisplayName").unwrap_or_default();
                let display_name = display_name.trim();
                let suffix = version_from_display_name(display_name, product);
                let version: Option<String> = entry.get_value("DisplayVersion").ok();
                let matches = display_name.eq_ignore_ascii_case(product)
                    || suffix.is_some()
                    || version.as_ref().is_some_and(|version| {
                        display_name.eq_ignore_ascii_case(&format!("{product} {version}"))
                    });
                if !matches {
                    continue;
                }
                let location: String = entry.get_value("InstallLocation").unwrap_or_default();
                let icon: String = entry.get_value("DisplayIcon").unwrap_or_default();
                if is_installed_executable(
                    &Path::new(location.trim_matches('"')).join(executable),
                    executable,
                ) || is_installed_executable(display_icon_path(&icon), executable)
                {
                    return (
                        true,
                        version
                            .filter(|value| !value.trim().is_empty())
                            .or_else(|| suffix.map(str::to_owned)),
                    );
                }
            }
        }
    }

    let user_programs = dirs::data_local_dir().map(|dir| dir.join("Programs"));
    let system_programs = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from);
    let installed = user_programs
        .into_iter()
        .chain(system_programs)
        .any(|dir| is_installed_executable(&dir.join(product).join(executable), executable));
    (installed, None)
}

pub(super) fn has_codex_desktop_package() -> bool {
    let family: Vec<u16> = "OpenAI.Codex_2p2nqsd0c76g0"
        .encode_utf16()
        .chain([0])
        .collect();
    let mut count = 0;
    let mut buffer_len = 0;
    // SAFETY: family is NUL-terminated and both zero-capacity output buffers are null.
    // The sizing call reports the number of packages registered for the current user.
    let result = unsafe {
        GetPackagesByPackageFamily(
            family.as_ptr(),
            &mut count,
            null_mut(),
            &mut buffer_len,
            null_mut(),
        )
    };
    count > 0 && result == ERROR_INSUFFICIENT_BUFFER
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn matches_names_with_the_nsis_version_suffix() {
        assert_eq!(
            version_from_display_name("DeepWrite 1.8.5", "DeepWrite"),
            Some("1.8.5")
        );
        assert_eq!(
            version_from_display_name("DeepSeek Harness v2.0.0-beta.1", "DeepSeek Harness"),
            Some("v2.0.0-beta.1")
        );
        assert_eq!(
            version_from_display_name("DeepWrite Setup", "DeepWrite"),
            None
        );
        assert_eq!(
            version_from_display_name("DeepWrite Helper 1.9.0", "DeepWrite"),
            None
        );
    }

    #[test]
    fn checks_executable_named_by_display_icon() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Custom, Directory").join("DeepWrite.exe");
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, []).unwrap();
        let icon = format!("\"{}\", -1", path.display());
        assert!(is_installed_executable(
            display_icon_path(&icon),
            "deepwrite.exe"
        ));
        assert!(!is_installed_executable(
            display_icon_path(&icon),
            "other.exe"
        ));
        fs::remove_file(&path).unwrap();
        assert!(!is_installed_executable(
            display_icon_path(&icon),
            "deepwrite.exe"
        ));
    }
}
