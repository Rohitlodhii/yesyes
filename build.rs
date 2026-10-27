use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc_exe() -> Option<PathBuf> {
    // Check PATH first
    if Command::new("rc.exe").arg("/?").output().is_ok() {
        return Some(PathBuf::from("rc.exe"));
    }

    // Check Windows SDK paths
    let base = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if base.exists() {
        if let Ok(entries) = std::fs::read_dir(base) {
            let mut versions: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .collect();
            versions.sort();
            for ver_dir in versions.into_iter().rev() {
                let rc = ver_dir.join("x64").join("rc.exe");
                if rc.exists() {
                    return Some(rc);
                }
            }
        }
    }
    None
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest_path = manifest_dir.join("snapdock.manifest");
    let rc_path = manifest_dir.join("snapdock.rc");
    let ico_path = manifest_dir.join("snapdock.ico");

    println!("cargo:rerun-if-changed={}", manifest_path.display());
    println!("cargo:rerun-if-changed={}", rc_path.display());
    println!("cargo:rerun-if-changed={}", ico_path.display());

    // Embed DPI-aware PerMonitorV2 manifest via MSVC linker
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest_path.display());

    // Compile and link .rc icon resource if rc.exe is found
    if let Some(rc_exe) = find_rc_exe() {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let res_path = out_dir.join("snapdock.res");
        let status = Command::new(&rc_exe)
            .current_dir(&manifest_dir)
            .arg("/fo")
            .arg(&res_path)
            .arg(&rc_path)
            .status();

        if let Ok(s) = status {
            if s.success() {
                println!("cargo:rustc-link-arg={}", res_path.display());
            }
        }
    }
}
