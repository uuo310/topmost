// 托盘图标 + 右键菜单。图标从 exe 自己的 RT_GROUP_ICON（id 1）按托盘尺寸取。
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 1;
pub const WM_PIN_CLICK: u32 = WM_APP + 2;

const ID_AUTORUN: usize = 1;
const ID_EXIT: usize = 2;
pub const ID_AUTO: usize = ID_AUTORUN;
pub const ID_QUIT: usize = ID_EXIT;

const TIP: &str = "置顶工具：Ctrl+Alt+T 置顶 / 点标题栏图钉取消";

pub struct Tray {
    nid: NOTIFYICONDATAW,
    icon: HICON,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        unsafe {
            let n = GetSystemMetrics(SM_CXSMICON).max(16);
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            // 资源 id 1：build.rs 里 rc 把 app.ico 编成 RT_GROUP_ICON 1
            let icon = LoadImageW(
                Some(HINSTANCE(hinst.0)),
                PCWSTR(1 as *const u16),
                IMAGE_ICON,
                n,
                n,
                LR_DEFAULTCOLOR,
            )
            .map(|h| HICON(h.0))
            .unwrap_or_default();

            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_TRAY,
                hIcon: icon,
                ..Default::default()
            };
            for (i, c) in TIP.encode_utf16().take(127).enumerate() {
                nid.szTip[i] = c;
            }
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);

            Self { nid, icon }
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

/// 弹右键菜单，返回选中的命令号（0 = 没选）
pub unsafe fn popup(hwnd: HWND, autorun_checked: bool) -> usize {
    let Ok(menu) = CreatePopupMenu() else {
        return 0;
    };
    let flag = MF_STRING | if autorun_checked { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(menu, flag, ID_AUTORUN, w!("开机自动启动"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("退出"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    // 菜单要在前台，否则点外面不会关
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    cmd.0 as usize
}
