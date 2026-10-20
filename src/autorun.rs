// 开机自启：写 HKCU 的 Run 键，值和 C# 版一致（带引号的 exe 全路径）。
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, REG_VALUE_TYPE,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "TopMost";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn command() -> String {
    let p = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("\"{p}\"")
}

unsafe fn open(write: bool) -> Option<HKEY> {
    let key = wide(RUN_KEY);
    let mut h = HKEY::default();
    let mut sam = KEY_READ;
    if write {
        sam |= KEY_WRITE;
    }
    let r = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), Some(0), sam, &mut h);
    if r.0 == 0 {
        Some(h)
    } else {
        None
    }
}

fn current_value() -> Option<String> {
    unsafe {
        let h = open(false)?;
        let name = wide(VALUE);
        let mut ty = REG_VALUE_TYPE::default();
        let mut cb = 0u32;
        let mut out = None;
        if RegQueryValueExW(h, PCWSTR(name.as_ptr()), None, Some(&mut ty), None, Some(&mut cb)).0 == 0
            && cb > 2
        {
            let mut buf = vec![0u8; cb as usize];
            if RegQueryValueExW(
                h,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut ty),
                Some(buf.as_mut_ptr()),
                Some(&mut cb),
            )
            .0 == 0
            {
                let u: Vec<u16> = buf
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&c| c != 0)
                    .collect();
                out = String::from_utf16(&u).ok();
            }
        }
        let _ = RegCloseKey(h);
        out
    }
}

/// 只有指向当前这个 exe 才算已开启
pub fn enabled() -> bool {
    current_value().is_some_and(|v| v.eq_ignore_ascii_case(&command()))
}

pub fn set(on: bool) {
    unsafe {
        let Some(h) = open(true) else { return };
        let name = wide(VALUE);
        if on {
            let data: Vec<u8> = command()
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(|c| c.to_le_bytes())
                .collect();
            let _ = RegSetValueExW(h, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data));
        } else {
            let _ = RegDeleteValueW(h, PCWSTR(name.as_ptr()));
        }
        let _ = RegCloseKey(h);
    }
}

/// 程序被挪了位置：只把「本来就开着」的那条改成当前路径，没开过就不碰。
pub fn follow() {
    if let Some(cur) = current_value() {
        if !cur.eq_ignore_ascii_case(&command()) {
            set(true);
        }
    }
}
