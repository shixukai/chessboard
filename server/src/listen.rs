use serde::Deserialize;
use serde::Serialize;
use xcap::image;
use xcap::image::GenericImage;

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

pub struct ListenWindow {
    window: xcap::Window,

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
                    return Some(Self { window, x: 0, y: 0, w: 0, h: 0 });
                }
            }
        }
        None
    }

    pub fn capture(&self) -> Option<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
        let mut pic = self.window.capture_image().ok()?;
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
