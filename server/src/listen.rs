use serde::Deserialize;
use serde::Serialize;
use xcap::image;
use xcap::image::GenericImage;
use xcap::image::ImageBuffer;
use xcap::image::Rgba;

#[derive(Serialize, Deserialize, Debug)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub width: u32,
    pub height: u32,
}

impl Window {
    pub fn try_new(win: &xcap::Window) -> Option<Self> {
        let id = win.id().ok()?;
        let title = win.title().ok().unwrap_or_default();
        let app_name = win.app_name().ok().unwrap_or_default();
        let width = win.width().ok().unwrap_or(0);
        let height = win.height().ok().unwrap_or(0);
        Some(Self { id, title, app_name, width, height })
    }
}

#[tauri::command]
pub async fn list_windows() -> Result<Vec<Window>, String> {
    let windows = xcap::Window::all().map_err(|e| format!("获取窗口失败: {}", e))?;
    if windows.is_empty() {
        return Err("no window".to_string());
    }
    let mut result = vec![];
    for window in windows.iter() {
        if let Some(w) = Window::try_new(window) {
            result.push(w);
        }
    }
    Ok(result)
}

/// 快速抽样判断图像是否为纯黑或无有效渲染内容
pub fn is_frame_blank(img: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> bool {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 {
        return true;
    }

    let step_x = (w / 16).max(1);
    let step_y = (h / 16).max(1);
    let mut non_black_count = 0;

    for y in (0..h).step_by(step_y as usize) {
        for x in (0..w).step_by(step_x as usize) {
            let p = img.get_pixel(x, y);
            // 只要有任何像素通道亮度超过阈值即算非全黑
            if p[0] > 15 || p[1] > 15 || p[2] > 15 {
                non_black_count += 1;
                if non_black_count >= 5 {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(target_os = "windows")]
mod win_capture {
    use super::*;
    use std::sync::atomic::{AtomicIsize, AtomicU8, Ordering};

    type HWND = isize;
    type HDC = isize;
    type HBITMAP = isize;
    type HGDIOBJ = isize;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct RECT {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct BITMAPINFOHEADER {
        pub bi_size: u32,
        pub bi_width: i32,
        pub bi_height: i32,
        pub bi_planes: u16,
        pub bi_bit_count: u16,
        pub bi_compression: u32,
        pub bi_size_image: u32,
        pub bi_x_pels_per_meter: i32,
        pub bi_y_pels_per_meter: i32,
        pub bi_clr_used: u32,
        pub bi_clr_important: u32,
    }

    #[repr(C)]
    pub struct BITMAPINFO {
        pub bmi_header: BITMAPINFOHEADER,
        pub bmi_colors: [u32; 1],
    }

    const PW_RENDERFULLCONTENT: u32 = 2;
    const SRCCOPY: u32 = 0x00CC0020;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;
    const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

    extern "system" {
        fn SetThreadDpiAwarenessContext(dpi_context: isize) -> isize;
        fn IsWindow(hwnd: HWND) -> i32;
        fn IsWindowVisible(hwnd: HWND) -> i32;
        fn IsIconic(hwnd: HWND) -> i32;
        fn GetClientRect(hwnd: HWND, lp_rect: *mut RECT) -> i32;
        fn ClientToScreen(hwnd: HWND, lp_point: *mut POINT) -> i32;
        fn GetClassNameW(hwnd: HWND, lp_class_name: *mut u16, n_max_count: i32) -> i32;
        fn EnumChildWindows(
            hwnd: HWND,
            lp_enum_func: Option<unsafe extern "system" fn(HWND, isize) -> i32>,
            l_param: isize,
        ) -> i32;
        fn PrintWindow(hwnd: HWND, hdc: HDC, n_flags: u32) -> i32;
        fn GetDC(hwnd: HWND) -> HDC;
        fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
        fn CreateCompatibleDC(hdc: HDC) -> HDC;
        fn DeleteDC(hdc: HDC) -> i32;
        fn CreateCompatibleBitmap(hdc: HDC, cx: i32, cy: i32) -> HBITMAP;
        fn SelectObject(hdc: HDC, hgdiobj: HGDIOBJ) -> HGDIOBJ;
        fn DeleteObject(hgdiobj: HGDIOBJ) -> i32;
        fn BitBlt(
            hdc_dest: HDC,
            x_dest: i32,
            y_dest: i32,
            w: i32,
            h: i32,
            hdc_src: HDC,
            x_src: i32,
            y_src: i32,
            rop: u32,
        ) -> i32;
        fn GetDIBits(
            hdc: HDC,
            hbm: HBITMAP,
            start: u32,
            lines: u32,
            lpv_bits: *mut u8,
            lpbmi: *mut BITMAPINFO,
            usage: u32,
        ) -> i32;
    }

    struct DcGuard {
        hwnd: HWND,
        hdc: HDC,
    }
    impl Drop for DcGuard {
        fn drop(&mut self) {
            if self.hdc != 0 {
                unsafe { ReleaseDC(self.hwnd, self.hdc); }
            }
        }
    }

    struct MemDcGuard {
        hdc: HDC,
    }
    impl Drop for MemDcGuard {
        fn drop(&mut self) {
            if self.hdc != 0 {
                unsafe { DeleteDC(self.hdc); }
            }
        }
    }

    struct BitmapGuard {
        hbmp: HBITMAP,
    }
    impl Drop for BitmapGuard {
        fn drop(&mut self) {
            if self.hbmp != 0 {
                unsafe { DeleteObject(self.hbmp); }
            }
        }
    }

    unsafe fn hbitmap_to_image(
        hdc: HDC,
        hbmp: HBITMAP,
        w: i32,
        h: i32,
    ) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        if w <= 0 || h <= 0 {
            return None;
        }

        let mut bmi = BITMAPINFO {
            bmi_header: BITMAPINFOHEADER {
                bi_size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                bi_width: w,
                bi_height: -h, // 负高度代表从上到下的 Top-down DIB
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: BI_RGB,
                bi_size_image: (w * h * 4) as u32,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            bmi_colors: [0; 1],
        };

        let mut buf: Vec<u8> = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(
            hdc,
            hbmp,
            0,
            h as u32,
            buf.as_mut_ptr(),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        if lines == 0 {
            return None;
        }

        // BGRA 转 RGBA 并强制设置 Alpha 为 255
        for chunk in buf.chunks_exact_mut(4) {
            let b = chunk[0];
            let r = chunk[2];
            chunk[0] = r;
            chunk[2] = b;
            chunk[3] = 255;
        }

        ImageBuffer::from_raw(w as u32, h as u32, buf)
    }

    /// 使用 PrintWindow 截取指定 HWND 窗口表面
    pub fn capture_hwnd_print_window(hwnd: HWND) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            if IsWindow(hwnd) == 0 {
                return None;
            }

            let mut rect = RECT::default();
            if GetClientRect(hwnd, &mut rect) == 0 {
                return None;
            }
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            if w <= 0 || h <= 0 {
                return None;
            }

            let raw_screen_dc = GetDC(0);
            if raw_screen_dc == 0 {
                return None;
            }
            let _screen_guard = DcGuard { hwnd: 0, hdc: raw_screen_dc };

            let raw_mem_dc = CreateCompatibleDC(raw_screen_dc);
            if raw_mem_dc == 0 {
                return None;
            }
            let _mem_guard = MemDcGuard { hdc: raw_mem_dc };

            let raw_bmp = CreateCompatibleBitmap(raw_screen_dc, w, h);
            if raw_bmp == 0 {
                return None;
            }
            let _bmp_guard = BitmapGuard { hbmp: raw_bmp };

            let h_old = SelectObject(raw_mem_dc, raw_bmp);
            let ok = PrintWindow(hwnd, raw_mem_dc, PW_RENDERFULLCONTENT);
            let result = if ok != 0 {
                hbitmap_to_image(raw_mem_dc, raw_bmp, w, h)
            } else {
                None
            };
            SelectObject(raw_mem_dc, h_old);

            result
        }
    }

    /// 使用屏幕 DC BitBlt 裁切窗口屏幕区域 (作为 Direct3D/OpenGL/Vulkan 终极防黑屏兜底)
    pub fn capture_hwnd_screen_blit(hwnd: HWND) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            if IsWindow(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return None;
            }

            let mut rect = RECT::default();
            if GetClientRect(hwnd, &mut rect) == 0 {
                return None;
            }
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            if w <= 0 || h <= 0 {
                return None;
            }

            let mut pt = POINT { x: rect.left, y: rect.top };
            if ClientToScreen(hwnd, &mut pt) == 0 {
                return None;
            }

            let raw_screen_dc = GetDC(0);
            if raw_screen_dc == 0 {
                return None;
            }
            let _screen_guard = DcGuard { hwnd: 0, hdc: raw_screen_dc };

            let raw_mem_dc = CreateCompatibleDC(raw_screen_dc);
            if raw_mem_dc == 0 {
                return None;
            }
            let _mem_guard = MemDcGuard { hdc: raw_mem_dc };

            let raw_bmp = CreateCompatibleBitmap(raw_screen_dc, w, h);
            if raw_bmp == 0 {
                return None;
            }
            let _bmp_guard = BitmapGuard { hbmp: raw_bmp };

            let h_old = SelectObject(raw_mem_dc, raw_bmp);
            let ok = BitBlt(raw_mem_dc, 0, 0, w, h, raw_screen_dc, pt.x, pt.y, SRCCOPY);
            let result = if ok != 0 {
                hbitmap_to_image(raw_mem_dc, raw_bmp, w, h)
            } else {
                None
            };
            SelectObject(raw_mem_dc, h_old);

            result
        }
    }

    unsafe extern "system" fn enum_child_proc(hwnd: HWND, l_param: isize) -> i32 {
        let list = &mut *(l_param as *mut Vec<HWND>);
        list.push(hwnd);
        1
    }

    /// 智能探测目标窗口下的所有渲染子窗口 (天天象棋 D3D / 安卓模拟器 / Qt / Web 渲染层)
    pub fn find_candidate_render_children(top_hwnd: HWND) -> Vec<HWND> {
        let mut children: Vec<HWND> = Vec::new();
        unsafe {
            EnumChildWindows(
                top_hwnd,
                Some(enum_child_proc),
                &mut children as *mut _ as isize,
            );
        }

        let mut scored_children: Vec<(HWND, i64)> = Vec::new();

        for child in children {
            unsafe {
                if IsWindowVisible(child) == 0 {
                    continue;
                }
                let mut rect = RECT::default();
                GetClientRect(child, &mut rect);
                let w = (rect.right - rect.left) as i64;
                let h = (rect.bottom - rect.top) as i64;
                if w < 100 || h < 100 {
                    continue;
                }

                let mut class_buf = [0u16; 256];
                let len = GetClassNameW(child, class_buf.as_mut_ptr(), 256);
                let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
                let lower_class = class_name.to_lowercase();

                let mut score = (w * h) / 1000;
                // 天天象棋 (QQ游戏大厅 / Electron D3D 渲染层)
                if lower_class.contains("intermediate d3d window") {
                    score += 10_000_000;
                }
                // 腾讯手游助手 / 安卓模拟器
                else if lower_class.contains("subwin") {
                    score += 8_000_000;
                }
                // Qt / 雷电 / MuMu 模拟器渲染表面
                else if lower_class.contains("renderwindow") || lower_class.contains("qt5qwindowicon") {
                    score += 5_000_000;
                }
                // Chrome / Edge 渲染视图
                else if lower_class.contains("chrome_renderwidgethosthwnd") {
                    score += 6_000_000;
                }

                scored_children.push((child, score));
            }
        }

        scored_children.sort_by(|a, b| b.1.cmp(&a.1));
        scored_children.into_iter().map(|(hwnd, _)| hwnd).collect()
    }

    pub struct WinTargetState {
        pub top_hwnd: HWND,
        pub active_hwnd: AtomicIsize,
        pub capture_mode: AtomicU8, // 0: Auto, 1: TopPrint, 2: ChildPrint, 3: ScreenBlit
    }

    impl WinTargetState {
        pub fn new(top_hwnd: HWND) -> Self {
            Self {
                top_hwnd,
                active_hwnd: AtomicIsize::new(top_hwnd),
                capture_mode: AtomicU8::new(0),
            }
        }

        pub fn capture_waterfall(&self) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
            let mode = self.capture_mode.load(Ordering::Relaxed);
            let active = self.active_hwnd.load(Ordering::Relaxed);

            // 如果已有稳定命中的模式，优先快速抓取
            if mode != 0 && active != 0 {
                let fast_img = match mode {
                    1 | 2 => capture_hwnd_print_window(active),
                    3 => capture_hwnd_screen_blit(active),
                    _ => None,
                };
                if let Some(img) = fast_img {
                    if !is_frame_blank(&img) {
                        return Some(img);
                    }
                }
                // 如果快速抓取出错或黑屏，重置进入自动重探链路
                self.capture_mode.store(0, Ordering::Relaxed);
            }

            // --- 级联捕获链路 (Waterfall Cascade) ---

            // 第 1 级: 尝试直接 PrintWindow 抓取顶层窗口 (适用经典 GDI 软件)
            if let Some(img) = capture_hwnd_print_window(self.top_hwnd) {
                if !is_frame_blank(&img) {
                    self.active_hwnd.store(self.top_hwnd, Ordering::Relaxed);
                    self.capture_mode.store(1, Ordering::Relaxed);
                    return Some(img);
                }
            }

            // 第 2 级: 智能寻靶探测子渲染窗口 (针对天天象棋 D3D / 模拟器等)
            let candidates = find_candidate_render_children(self.top_hwnd);
            for child_hwnd in candidates.iter().copied() {
                if let Some(img) = capture_hwnd_print_window(child_hwnd) {
                    if !is_frame_blank(&img) {
                        tracing::info!("自适应命中最佳子渲染窗口: HWND 0x{:X}", child_hwnd);
                        self.active_hwnd.store(child_hwnd, Ordering::Relaxed);
                        self.capture_mode.store(2, Ordering::Relaxed);
                        return Some(img);
                    }
                }
            }

            // 第 3 级: 桌面视口裁切兜底 (针对 Direct3D/OpenGL/Vulkan 纯硬件交换链)
            let target_for_blit = if !candidates.is_empty() {
                candidates[0]
            } else {
                self.top_hwnd
            };

            if let Some(img) = capture_hwnd_screen_blit(target_for_blit) {
                if !is_frame_blank(&img) {
                    tracing::info!("自适应进入桌面视口裁切模式: HWND 0x{:X}", target_for_blit);
                    self.active_hwnd.store(target_for_blit, Ordering::Relaxed);
                    self.capture_mode.store(3, Ordering::Relaxed);
                    return Some(img);
                }
            }

            None
        }
    }
}

pub struct ListenWindow {
    window: xcap::Window,
    #[cfg(target_os = "windows")]
    win_state: win_capture::WinTargetState,

    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl ListenWindow {
    #[tracing::instrument]
    pub fn new(target: &Window, _w: usize, _h: usize) -> Option<Self> {
        let windows = xcap::Window::all().ok()?;
        for window in windows {
            if let Ok(id) = window.id() {
                if id == target.id {
                    return Some(Self {
                        window,
                        #[cfg(target_os = "windows")]
                        win_state: win_capture::WinTargetState::new(target.id as isize),
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    });
                }
            }
        }
        None
    }

    pub fn capture(&self) -> Option<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
        #[cfg(target_os = "windows")]
        let raw_img = self.win_state.capture_waterfall().or_else(|| self.window.capture_image().ok());

        #[cfg(not(target_os = "windows"))]
        let raw_img = self.window.capture_image().ok();

        let mut pic = raw_img?;

        if self.w > 0 && self.h > 0 {
            let img_w = pic.width();
            let img_h = pic.height();
            if self.x + self.w <= img_w && self.y + self.h <= img_h {
                pic = pic.sub_image(self.x, self.y, self.w, self.h).to_image();
            } else {
                tracing::warn!(
                    "sub_bound out of bounds: crop ({}, {}, {}, {}) vs image ({}, {})",
                    self.x, self.y, self.w, self.h, img_w, img_h
                );
                return None;
            }
        }
        Some(pic)
    }

    pub fn set_sub_bound(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.x = x;
        self.y = y;
        self.w = w;
        self.h = h;
    }
}
