//! Application storage paths. Provider credentials keep their own locations.

use std::ffi::OsString;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    resolve_data_dir(|name| std::env::var_os(name))
}

fn adopt_legacy_data_dir(current: PathBuf, legacy: PathBuf) -> PathBuf {
    if current.exists() || !legacy.exists() {
        return current;
    }
    if std::fs::rename(&legacy, &current).is_ok() {
        current
    } else {
        legacy
    }
}

fn resolve_data_dir(mut env: impl FnMut(&str) -> Option<OsString>) -> PathBuf {
    if let Some(dir) = env("ZEREN_DATA_DIR").or_else(|| env("ZERON_DATA_DIR")) {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        // Explorer does not set HOME. Do not let a shell-specific HOME select
        // a different workspace from a desktop launch, or migrate credentials
        // between Unix-style and native Windows directories implicitly.
        let local = env("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env("USERPROFILE")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join("AppData").join("Local"))
            })
            .expect("LOCALAPPDATA and USERPROFILE not set; set ZEREN_DATA_DIR");
        adopt_legacy_data_dir(local.join("Zeren"), local.join("Zeron"))
    }
    #[cfg(not(windows))]
    {
        let home = PathBuf::from(env("HOME").expect("HOME not set"));
        let dir = home.join(".zeren");
        if home.join(".zeron").exists() {
            adopt_legacy_data_dir(dir, home.join(".zeron"))
        } else {
            adopt_legacy_data_dir(dir, home.join(".comet-native"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(vars: &[(&str, &str)]) -> PathBuf {
        resolve_data_dir(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.into())
        })
    }

    #[test]
    fn explicit_data_dir_needs_no_home() {
        assert_eq!(
            resolve(&[("ZEREN_DATA_DIR", "custom data")]),
            PathBuf::from("custom data")
        );
    }

    #[test]
    fn legacy_data_dir_is_adopted() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("Zeron");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("chat.txt"), "saved").unwrap();
        let current = adopt_legacy_data_dir(root.path().join("Zeren"), legacy);
        assert_eq!(
            std::fs::read_to_string(current.join("chat.txt")).unwrap(),
            "saved"
        );
        assert_eq!(current, root.path().join("Zeren"));
    }

    #[cfg(windows)]
    #[test]
    fn explorer_launch_without_home_uses_local_app_data() {
        assert_eq!(
            resolve(&[("LOCALAPPDATA", r"C:\Users\Test User\AppData\Local")]),
            PathBuf::from(r"C:\Users\Test User\AppData\Local\Zeren"),
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_profile_fallback_handles_unicode_and_apostrophes() {
        assert_eq!(
            resolve(&[("USERPROFILE", r"C:\Users\O'Brien 日本語")]),
            PathBuf::from(r"C:\Users\O'Brien 日本語\AppData\Local\Zeren"),
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_default_does_not_depend_on_shell_home() {
        assert_eq!(
            resolve(&[("HOME", r"D:\msys-home"), ("LOCALAPPDATA", r"C:\Local")]),
            PathBuf::from(r"C:\Local\Zeren"),
        );
    }
}
