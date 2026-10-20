// 把 app.ico 编成 Win32 资源（RT_GROUP_ICON id 1），这样 exe 在资源管理器里才有图标。
// 不用额外 crate：直接调 Windows SDK 的 rc.exe，再把 .res 交给链接器。
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RC") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in std::fs::read_dir(kits).ok()?.flatten() {
        let dir = entry.path();
        let exe = dir.join("x64").join("rc.exe");
        if !exe.is_file() {
            continue;
        }
        let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let ver: Vec<u32> = name
            .split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect();
        if best.as_ref().map_or(true, |(v, _)| ver > *v) {
            best = Some((ver, exe));
        }
    }
    best.map(|(_, p)| p)
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest.join("app.ico");
    println!("cargo:rerun-if-changed=app.ico");
    if !ico.is_file() {
        return;
    }

    let Some(rc_exe) = find_rc() else {
        println!("cargo:warning=没找到 rc.exe，exe 将没有文件图标（功能不受影响）");
        return;
    };

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc_file = out.join("app.rc");
    let res_file = out.join("app.res");
    std::fs::write(&rc_file, "1 ICON \"app.ico\"\n").unwrap();

    let ok = Command::new(rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .current_dir(&manifest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        println!("cargo:rustc-link-arg={}", res_file.display());
    } else {
        println!("cargo:warning=rc.exe 失败，exe 将没有文件图标");
    }
}
