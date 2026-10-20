#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// topmost（Rust 版）——Ctrl+Shift+A 置顶当前窗口，标题栏出现图钉，点图钉取消。
// 纯 Win32：热键、分层窗口、DWM 定位、自绘图标，无第三方 UI 依赖。

mod autorun;
mod pin;
mod tray;

use std::collections::HashMap;

use windows::core::w;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CAPTION_BUTTON_BOUNDS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, TrackMouseEvent, UnregisterHotKey, MOD_CONTROL, MOD_SHIFT, TME_LEAVE,
    TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const MAIN_CLASS: windows::core::PCWSTR = w!("TopMostRustMain");
const HOTKEY_ID: i32 = 0xA17;
const TIMER_ID: usize = 1;
// 热键 Ctrl+Shift+A。一路换过来的原因：
// Win+Ctrl+T —— PowerToys「始终置顶」的默认键，必然撞车；
// Win+Shift+字母 里 A C M P R S T V W 被系统占着，Win+Alt+字母 被 Xbox Game Bar 占着；
// Ctrl+Alt+字母 扫起来虽然全空，但 Ctrl+Alt 是太多老软件/显卡面板的传统热键区，实测照样被抢。
// Ctrl+Shift+字母 用 RegisterHotKey 扫一遍也是整片空闲，且这类组合用的人少。
// 要是哪天又被输入法吃掉：把这里换成别的键。Ctrl+Shift 是中文输入法传统占用的区域。
const VK_A: u32 = 0x41;

struct PinState {
    hwnd: HWND,
    size: i32,
    hover: bool,
    x: i32,
    y: i32,
    shown: bool,
    tracking: bool,
}

struct App {
    main: HWND,
    pins: HashMap<usize, PinState>,
    tray: Option<tray::Tray>,
}

static mut APP: *mut App = std::ptr::null_mut();

fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

fn key(h: HWND) -> usize {
    h.0 as usize
}

fn hwnd_of(k: usize) -> HWND {
    HWND(k as *mut core::ffi::c_void)
}

fn is_topmost(h: HWND) -> bool {
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }
}

fn has_caption(h: HWND) -> bool {
    unsafe { GetWindowLongPtrW(h, GWL_STYLE) as u32 & WS_CAPTION.0 != 0 }
}

fn set_topmost(h: HWND, on: bool) {
    unsafe {
        let _ = SetWindowPos(
            h,
            Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }),
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
        );
    }
}

impl App {
    fn toggle_foreground(&mut self) {
        unsafe {
            let h = GetForegroundWindow();
            if h.is_invalid() || h == self.main {
                return;
            }
            if self.pins.contains_key(&key(h)) {
                self.unpin(h);
            } else {
                self.pin(h);
            }
        }
    }

    fn pin(&mut self, target: HWND) {
        unsafe {
            if !IsWindow(Some(target)).as_bool() || self.pins.contains_key(&key(target)) {
                return;
            }
            set_topmost(target, true);
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let Some(ph) = pin::create(target, HINSTANCE(hinst.0)) else {
                return;
            };
            self.pins.insert(
                key(target),
                PinState {
                    hwnd: ph,
                    size: 0,
                    hover: false,
                    x: i32::MIN,
                    y: i32::MIN,
                    shown: false,
                    tracking: false,
                },
            );
            self.update_badge(target);
        }
    }

    fn unpin(&mut self, target: HWND) {
        if let Some(p) = self.pins.remove(&key(target)) {
            pin::destroy(p.hwnd);
        }
        if unsafe { IsWindow(Some(target)).as_bool() } {
            set_topmost(target, false);
        }
    }

    /// 把图钉摆到标题栏、最小化按钮左边
    fn update_badge(&mut self, target: HWND) {
        let Some(p) = self.pins.get_mut(&key(target)) else {
            return;
        };
        unsafe {
            let visible = IsWindow(Some(target)).as_bool()
                && IsWindowVisible(target).as_bool()
                && !IsIconic(target).as_bool()
                && has_caption(target);
            if !visible {
                if p.shown {
                    pin::hide(p.hwnd);
                    p.shown = false;
                }
                return;
            }

            let mut wr = RECT::default();
            if GetWindowRect(target, &mut wr).is_err() {
                return;
            }
            let dpi = GetDpiForWindow(target);
            let scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };

            let (mut cap_h, mut cap_top, mut group_left) = (0i32, 0i32, 0i32);
            let mut ok = false;
            let mut cb = RECT::default();
            if DwmGetWindowAttribute(
                target,
                DWMWA_CAPTION_BUTTON_BOUNDS,
                &mut cb as *mut RECT as *mut core::ffi::c_void,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_ok()
                && cb.right > cb.left
            {
                cap_h = cb.bottom - cb.top;
                cap_top = wr.top + cb.top;
                group_left = wr.right - cb.right;
                // 落到窗口左半边说明这个矩形不可信
                ok = group_left >= wr.left + ((wr.right - wr.left) as f32 * 0.55) as i32;
            }
            if !ok {
                // 自绘标题栏（Chrome/Electron、UWP、Qt）给不出矩形，按三个系统按钮各 46 逻辑像素估
                cap_h = (32.0 * scale).round() as i32;
                cap_top = wr.top;
                group_left = wr.right - (138.0 * scale).round() as i32;
            }

            let mut size = (22.0 * scale).round() as i32;
            if cap_h > 0 {
                size = size.min((cap_h - 2).max(16));
            }
            let mut x = group_left - size - (2.0 * scale).round() as i32;
            let y = cap_top + (cap_h - size) / 2;
            let min_x = wr.left + (8.0 * scale).round() as i32;
            let max_x = wr.right - size - (8.0 * scale).round() as i32;
            x = x.clamp(min_x, max_x);

            if size != p.size {
                p.size = size;
                p.x = x;
                p.y = y;
                pin::paint(p.hwnd, x, y, size, p.hover);
            } else if x != p.x || y != p.y {
                p.x = x;
                p.y = y;
                pin::move_to(p.hwnd, x, y);
            }
            if !p.shown {
                pin::show(p.hwnd);
                p.shown = true;
            }
            let _ = SetWindowPos(
                p.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn sync_all(&mut self) {
        if self.pins.is_empty() {
            return;
        }
        let keys: Vec<usize> = self.pins.keys().copied().collect();
        for k in keys {
            let live = unsafe { IsWindow(Some(hwnd_of(k))).as_bool() } && is_topmost(hwnd_of(k));
            if !live {
                self.unpin(hwnd_of(k));
                continue;
            }
            // 窗口被点活后会盖住图钉，这里顺手把图钉提回置顶队首
            self.update_badge(hwnd_of(k));
        }
    }

    fn set_hover(&mut self, pin_hwnd: HWND, on: bool) {
        let Some(p) = self.pins.values_mut().find(|p| p.hwnd == pin_hwnd) else {
            return;
        };
        if on && !p.tracking {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: pin_hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            p.tracking = true;
        }
        if !on {
            p.tracking = false;
        }
        if p.hover == on {
            return;
        }
        p.hover = on;
        let (x, y, size, hover, ph) = (p.x, p.y, p.size, p.hover, p.hwnd);
        if size > 0 {
            pin::paint(ph, x, y, size, hover);
        }
    }

    fn on_click(&mut self, pin_hwnd: HWND) {
        let Some(&target) = self
            .pins
            .iter()
            .find(|(_, p)| p.hwnd == pin_hwnd)
            .map(|(k, _)| k)
        else {
            return;
        };
        // 延到下一轮消息再收窗口，别在自己的消息处理里销毁自己
        unsafe {
            let _ = PostMessageW(
                Some(self.main),
                tray::WM_PIN_CLICK,
                WPARAM(target),
                LPARAM(0),
            );
        }
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            if wp.0 as i32 == HOTKEY_ID {
                app().toggle_foreground();
            }
            LRESULT(0)
        }
        WM_TIMER => {
            app().sync_all();
            LRESULT(0)
        }
        tray::WM_TRAY => {
            // 老式回调：lParam 低字是鼠标消息
            let ev = (lp.0 & 0xFFFF) as u32;
            if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                let checked = autorun::enabled();
                match tray::popup(hwnd, checked) {
                    tray::ID_AUTO => autorun::set(!checked),
                    tray::ID_QUIT => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        tray::WM_PIN_CLICK => {
            app().unpin(hwnd_of(wp.0));
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            // 托盘图标必须赶在这个窗口句柄失效前摘掉（NIM_DELETE 靠 hWnd+uID 定位），
            // 放到收尾阶段再摘，句柄已经没了，图标会残留在通知区直到鼠标划过
            if !APP.is_null() {
                app().tray = None;
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn win_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object == OBJID_WINDOW.0 && !hwnd.is_invalid() && !APP.is_null() {
        let a = app();
        if a.pins.contains_key(&key(hwnd)) {
            a.update_badge(hwnd);
        }
    }
}

// 图钉窗口的两个回调，由 pin.rs 转发过来
pub fn on_pin_hover(pin_hwnd: HWND, on: bool) {
    unsafe {
        if !APP.is_null() {
            app().set_hover(pin_hwnd, on);
        }
    }
}

pub fn on_pin_click(pin_hwnd: HWND) {
    unsafe {
        if !APP.is_null() {
            app().on_click(pin_hwnd);
        }
    }
}

fn main() {
    unsafe {
        // 单实例
        let _mtx = CreateMutexW(None, true, w!("Local\\TopMostRustSingle"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let hi = HINSTANCE(hinst.0);

        let wc = WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: hi,
            lpszClassName: MAIN_CLASS,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 || !pin::register(hi) {
            return;
        }

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            MAIN_CLASS,
            w!(""),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hi),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        if RegisterHotKey(Some(hwnd), HOTKEY_ID, MOD_CONTROL | MOD_SHIFT, VK_A).is_err() {
            MessageBoxW(
                Some(hwnd),
                w!("Ctrl+Shift+A 已被别的程序占用，置顶工具无法启动。\n关掉占用它的程序后再试，或改源码里的热键常量重编。"),
                w!("置顶工具"),
                MB_OK | MB_ICONERROR,
            );
            return;
        }

        autorun::follow(); // 程序被挪过位置的话，先把自启登记改过来

        let hook = SetWinEventHook(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            None,
            Some(win_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        let mut state = Box::new(App {
            main: hwnd,
            pins: HashMap::new(),
            tray: None,
        });
        state.tray = Some(tray::Tray::new(hwnd));
        // into_raw 把所有权交给 APP；不能再留着一个 Box 局部变量，
        // 否则 main 结束时会再 free 一次（收尾已经 from_raw 释放过了）→ 堆损坏 0xC0000374
        APP = Box::into_raw(state);

        let _ = SetTimer(Some(hwnd), TIMER_ID, 400, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 收尾：主窗口已在 WM_DESTROY 里销毁、托盘图标也在那里摘掉了，
        // 这里只剩图钉、定时器、热键、事件钩子和 APP 本体
        let a = app();
        let keys: Vec<usize> = a.pins.keys().copied().collect();
        for k in keys {
            a.unpin(hwnd_of(k));
        }
        a.tray = None; // WM_DESTROY 里已置空，这里是幂等的保险
        let _ = KillTimer(Some(hwnd), TIMER_ID);
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        if !hook.is_invalid() {
            let _ = UnhookWinEvent(hook);
        }
        drop(Box::from_raw(APP));
        APP = std::ptr::null_mut();
    }
}
