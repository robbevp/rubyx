use std::path::PathBuf;
use std::process::Command;

pub fn find_libpython() -> Option<PathBuf> {
    if let Some(path) = find_via_python_config() {
        if path.exists() {
            return Some(path);
        }
    }

    for path in common_paths() {
        if path.exists() {
            return Some(path);
        }
    }

    None
}

fn find_via_python_config() -> Option<PathBuf> {
    let output = Command::new("python3")
        .args([
            "-c",
            "import sysconfig; print(sysconfig.get_config_var('LIBDIR'))",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let libdir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if libdir.is_empty() || libdir == "None" {
        return None;
    }

    let version_output = Command::new("python3")
        .args([
            "-c",
            "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')",
        ])
        .output()
        .ok()?;

    let version = String::from_utf8_lossy(&version_output.stdout)
        .trim()
        .to_string();

    #[cfg(target_os = "macos")]
    let lib_name = format!("libpython{}.dylib", version);
    #[cfg(target_os = "linux")]
    let lib_name = format!("libpython{}.so", version);
    #[cfg(target_os = "windows")]
    let lib_name = format!("python{}.dll", version.replace(".", ""));
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let lib_name = format!("libpython{}.so", version);

    let path = PathBuf::from(libdir).join(&lib_name);
    if path.exists() {
        return Some(path);
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(p) = find_macos_framework(&version) {
            if p.exists() {
                return Some(p);
            }
        }
    }

    Some(path)
}

#[cfg(target_os = "macos")]
fn find_macos_framework(version: &str) -> Option<PathBuf> {
    let output = Command::new("python3")
        .args(["-c", "import sys; print(sys.prefix)"])
        .output()
        .ok()?;

    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let framework_lib = PathBuf::from(&prefix)
        .join("lib")
        .join(format!("libpython{}.dylib", version));
    if framework_lib.exists() {
        return Some(framework_lib);
    }

    let python_dylib = PathBuf::from(&prefix).join("Python");
    if python_dylib.exists() {
        return Some(python_dylib);
    }

    None
}

fn common_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    for minor in (8..=15).rev() {
        paths.push(PathBuf::from(format!(
            "/opt/homebrew/opt/python@3.{}/Frameworks/Python.framework/Versions/3.{}/lib/libpython3.{}.dylib",
            minor, minor, minor
        )));
        paths.push(PathBuf::from(format!(
            "/opt/homebrew/opt/python@3.{}/Frameworks/Python.framework/Versions/3.{}/Python",
            minor, minor
        )));
    }

    #[cfg(target_os = "linux")]
    for minor in (8..=15).rev() {
        paths.push(PathBuf::from(format!(
            "/usr/lib/x86_64-linux-gnu/libpython3.{}.so",
            minor
        )));
        paths.push(PathBuf::from(format!("/usr/lib/libpython3.{}.so", minor)));
    }

    paths
}
