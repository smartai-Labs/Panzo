use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-changed=../panzo-windows/assets/panzo.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../panzo-windows/assets/panzo.ico");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let rc = out.join("panzo.rc");
    let resource = out.join("panzo.res");
    let escaped = icon.to_string_lossy().replace('\\', "\\\\");
    fs::write(&rc, format!("101 ICON \"{escaped}\"\n")).expect("Write application icon resource");
    let compiler = resource_compiler()
        .expect("Windows SDK rc.exe was not found. Install the Windows SDK or set RC to rc.exe.");
    let result = Command::new(&compiler)
        .args(["/nologo", "/c", "65001", "/fo"])
        .arg(&resource)
        .arg(&rc)
        .output()
        .expect("Run Windows resource compiler");
    assert!(
        result.status.success(),
        "Application icon resource compilation failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    // Embedding the icon also covers Explorer, shortcuts and pinned taskbar items.
    for binary in ["panzo", "panzo-cli"] {
        println!("cargo:rustc-link-arg-bin={binary}={}", resource.display());
    }
}

fn resource_compiler() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("RC") {
        return Some(PathBuf::from(explicit));
    }
    if let Some(path) = env::var_os("PATH") {
        if let Some(found) = env::split_paths(&path)
            .map(|p| p.join("rc.exe"))
            .find(|p| p.is_file())
        {
            return Some(found);
        }
    }
    let sdk = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("Windows Kits/10"))
        })?;
    let bin = sdk.join("bin");
    let mut versions: Vec<_> = fs::read_dir(&bin)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("10."))
        .map(|entry| entry.path())
        .collect();
    versions.sort_by_key(|p| {
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .split('.')
            .filter_map(|part| part.parse::<u32>().ok())
            .collect::<Vec<_>>()
    });
    versions.reverse();
    versions.push(bin);
    let architectures = if cfg!(target_arch = "aarch64") {
        ["arm64", "x64", "x86"]
    } else {
        ["x64", "x86", "arm64"]
    };
    versions
        .into_iter()
        .flat_map(|version| architectures.map(|arch| version.join(arch).join("rc.exe")))
        .find(|p| p.is_file())
}
