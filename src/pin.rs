// 标题栏图钉：分层窗口 + 自己光栅化（4×4 超采样、解析式几何），不依赖 GDI+。
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ,
    ReleaseDC, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const CLASS: PCWSTR = w!("TopMostRustPin");

// windows crate 把它放在 Win32::UI::Controls 里，为一条常量多开一个 feature 不划算
const WM_MOUSELEAVE: u32 = 0x02A3;

// 13 单位坐标系里的三块几何：扁圆帽、横挡片、收尖的针
const CAP: (f32, f32, f32, f32) = (0.0, -3.20, 2.05, 1.85); // cx cy rx ry
const BAR: (f32, f32, f32) = (1.85, -1.05, 0.50); // 半宽 + 半径，中心 y
const NEEDLE: [(f32, f32); 4] = [
    (-0.50, -1.00),
    (0.50, -1.00),
    (0.20, 5.40),
    (-0.20, 5.40),
];

fn in_pin(lx: f32, ly: f32) -> bool {
    let (cx, cy, rx, ry) = CAP;
    let ex = (lx - cx) / rx;
    let ey = (ly - cy) / ry;
    if ex * ex + ey * ey <= 1.0 {
        return true;
    }
    let (hx, by, r) = BAR;
    if lx.abs() <= hx && ly >= by - r && ly <= by + r {
        return true;
    }
    for sign in [-1.0f32, 1.0] {
        let dx = lx - sign * hx;
        let dy = ly - by;
        if dx * dx + dy * dy <= r * r {
            return true;
        }
    }
    // 针：凹多边形？其实是细长的梯形，用同号叉积判内外
    let mut sign = 0.0;
    for i in 0..4 {
        let (x1, y1) = NEEDLE[i];
        let (x2, y2) = NEEDLE[(i + 1) % 4];
        let cross = (x2 - x1) * (ly - y1) - (y2 - y1) * (lx - x1);
        if cross.abs() < 1e-9 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn in_round_rect(px: f32, py: f32, lo: f32, hi: f32, r: f32) -> bool {
    if px < lo || px > hi || py < lo || py > hi {
        return false;
    }
    let cx = px.clamp(lo + r, hi - r);
    let cy = py.clamp(lo + r, hi - r);
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= r * r
}

/// 返回 BGRA（预乘）像素，自上而下
pub fn raster(size: i32, hover: bool) -> Vec<u32> {
    let n = size.max(1) as usize;
    let mut out = vec![0u32; n * n];
    let k = size as f32 / 13.0;
    let c = std::f32::consts::FRAC_1_SQRT_2;
    let half = size as f32 / 2.0;
    let lo = (size as f32 * 0.14).round().max(1.0);
    let hi = size as f32 - lo;

    let (pr, pg, pb) = if hover { (70.0, 205.0, 218.0) } else { (0.0, 183.0, 195.0) };
    let bg_a = 40.0 / 255.0;
    let samples = 4;
    let step = 1.0 / (samples * samples) as f32;

    for y in 0..n {
        for x in 0..n {
            let mut a_pin = 0.0f32;
            let mut a_bg = 0.0f32;
            for sy in 0..samples {
                for sx in 0..samples {
                    let px = x as f32 + (sx as f32 + 0.5) / samples as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / samples as f32;
                    if hover && in_round_rect(px, py, lo, hi, 4.0) {
                        a_bg += step;
                    }
                    let dx = px - half;
                    let dy = py - half;
                    // 逆旋转 45°：设备坐标 → 图钉局部坐标
                    let lx = (dx * c + dy * c) / k;
                    let ly = (-dx * c + dy * c) / k;
                    if in_pin(lx, ly) {
                        a_pin += step;
                    }
                }
            }
            let bg = a_bg * bg_a;
            let a = a_pin + bg * (1.0 - a_pin);
            if a < 0.0005 {
                continue;
            }
            let r = pr * a_pin + 128.0 * bg * (1.0 - a_pin);
            let g = pg * a_pin + 128.0 * bg * (1.0 - a_pin);
            let b = pb * a_pin + 128.0 * bg * (1.0 - a_pin);
            let av = (a * 255.0).round().clamp(0.0, 255.0) as u32;
            out[y * n + x] = (av << 24)
                | ((r.round().clamp(0.0, 255.0) as u32) << 16)
                | ((g.round().clamp(0.0, 255.0) as u32) << 8)
                | (b.round().clamp(0.0, 255.0) as u32);
        }
    }
    out
}

pub fn register(hinst: HINSTANCE) -> bool {
    unsafe {
        let wc = WNDCLASSW {
            style: WNDCLASS_STYLES(0),
            lpfnWndProc: Some(proc_),
            hInstance: hinst,
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&wc) != 0
    }
}

pub fn create(target: HWND, hinst: HINSTANCE) -> Option<HWND> {
    unsafe {
        let h = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            CLASS,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinst),
            None,
        )
        .ok()?;
        SetWindowLongPtrW(h, GWLP_USERDATA, target.0 as isize);
        Some(h)
    }
}

pub fn destroy(h: HWND) {
    unsafe {
        let _ = DestroyWindow(h);
    }
}

pub fn show(h: HWND) {
    unsafe {
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn hide(h: HWND) {
    unsafe {
        let _ = ShowWindow(h, SW_HIDE);
    }
}

/// 尺寸或悬停态变了才重推位图；只是挪位置用 SetWindowPos。
pub fn paint(h: HWND, x: i32, y: i32, size: i32, hover: bool) {
    let pixels = raster(size, hover);
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // 负高度＝自上而下
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(dib) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            std::ptr::copy_nonoverlapping(pixels.as_ptr() as *const u8, bits as *mut u8, pixels.len() * 4);
            let old = SelectObject(mem, HGDIOBJ(dib.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let dst = POINT { x, y };
            let sz = SIZE { cx: size, cy: size };
            let src = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(
                h,
                Some(screen),
                Some(&dst),
                Some(&sz),
                Some(mem),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(dib.0));
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

pub fn move_to(h: HWND, x: i32, y: i32) {
    unsafe {
        let _ = SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => crate::on_pin_hover(hwnd, true),
        WM_MOUSELEAVE => crate::on_pin_hover(hwnd, false),
        WM_LBUTTONUP => crate::on_pin_click(hwnd),
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ascii(size: i32, hover: bool) -> String {
        let px = raster(size, hover);
        let n = size as usize;
        let mut s = String::new();
        for y in 0..n {
            for x in 0..n {
                let a = (px[y * n + x] >> 24) & 0xFF;
                s.push(match a {
                    0..=31 => ' ',
                    32..=95 => '.',
                    96..=175 => '+',
                    _ => '#',
                });
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn pin_geometry() {
        let size = 33;
        let px = raster(size, false);
        let n = size as usize;
        let (mut x0, mut x1, mut y0, mut y1) = (n, 0usize, n, 0usize);
        let mut opaque = 0usize;
        for y in 0..n {
            for x in 0..n {
                if (px[y * n + x] >> 24) & 0xFF > 127 {
                    opaque += 1;
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
        }
        println!("{}", ascii(size, false));
        println!("bbox {}x{} opaque={}", x1 - x0 + 1, y1 - y0 + 1, opaque);
        assert!((16..=22).contains(&(x1 - x0 + 1)), "视觉宽度应在 19 左右");
        assert!((16..=22).contains(&(y1 - y0 + 1)), "视觉高度应在 19 左右");
    }

    #[test]
    fn hover_is_lighter() {
        let a = raster(33, false);
        let b = raster(33, true);
        let sum = |v: &[u32]| v.iter().map(|p| (p >> 16) & 0xFF).sum::<u32>();
        assert!(sum(&b) > sum(&a), "悬停应当更亮");
    }
}
