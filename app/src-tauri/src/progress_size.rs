//! One logical size for progress windows, independent of the download and display scale.

use std::{cell::Cell, fs, io, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalSize, Manager as _, PhysicalSize, WebviewWindow, WindowEvent};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
}

pub(crate) const MINIMUM: Size = Size {
    width: 460,
    height: 250,
};

impl Size {
    fn from_physical(size: PhysicalSize<u32>, scale: f64) -> Self {
        let LogicalSize { width, height } = size.to_logical(scale);
        Self { width, height }
    }

    fn fit(self, work_area: Option<Self>) -> Self {
        let available = work_area.unwrap_or(Self {
            width: u32::MAX,
            height: u32::MAX,
        });
        Self {
            width: self
                .width
                .clamp(MINIMUM.width, available.width.max(MINIMUM.width)),
            height: self
                .height
                .clamp(MINIMUM.height, available.height.max(MINIMUM.height)),
        }
    }
}

struct SavedSize {
    size: Size,
    dirty: bool,
}

pub(crate) struct Preference {
    path: PathBuf,
    state: Mutex<SavedSize>,
}

impl Preference {
    pub(crate) fn load(path: PathBuf) -> Self {
        let stored: Option<Size> = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let size = stored
            .filter(|s| s.width > 0 && s.height > 0)
            .unwrap_or(MINIMUM);
        Self {
            path,
            state: Mutex::new(SavedSize { size, dirty: false }),
        }
    }

    fn latest(&self) -> Size {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).size
    }

    pub(crate) fn initial_size(&self, app: &AppHandle) -> Size {
        let work_area =
            app.primary_monitor().ok().flatten().map(|monitor| {
                Size::from_physical(monitor.work_area().size, monitor.scale_factor())
            });
        self.latest().fit(work_area)
    }

    fn remember(&self, size: Size) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.size != size {
            *state = SavedSize { size, dirty: true };
        }
    }

    fn write(&self) -> io::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.dirty {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temp = self.path.with_extension("tmp");
        fs::write(&temp, serde_json::to_vec(&state.size)?)?;
        fs::rename(temp, &self.path)?;
        state.dirty = false;
        Ok(())
    }
}

pub(crate) fn watch(window: &WebviewWindow, initial: Size) {
    let previous = Cell::new(initial);
    let popup = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(size)
            if size.width > 0
                && size.height > 0
                && !popup.is_minimized().unwrap_or(false)
                && !popup.is_maximized().unwrap_or(false)
                && !popup.is_fullscreen().unwrap_or(false) =>
        {
            if let Ok(scale) = popup.scale_factor() {
                let size = Size::from_physical(*size, scale);
                if previous.replace(size) != size {
                    popup.state::<Preference>().remember(size);
                }
            }
        }
        WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => {
            previous.set(Size::from_physical(*new_inner_size, *scale_factor));
        }
        WindowEvent::CloseRequested { .. } => save(popup.app_handle()),
        _ => {}
    });
}

pub(crate) fn save(app: &AppHandle) {
    if let Some(preference) = app.try_state::<Preference>() {
        if let Err(error) = preference.write() {
            tracing::warn!(%error, "could not save the progress window size");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_size_across_sessions() {
        let dir = std::env::temp_dir().join(format!("oxyd-progress-{}", uuid::Uuid::new_v4()));
        let path = dir.join("size.json");
        let preference = Preference::load(path.clone());
        assert_eq!(preference.latest(), MINIMUM);
        preference.remember(Size {
            width: 620,
            height: 340,
        });
        preference.write().unwrap();
        let latest = Size {
            width: 650,
            height: 380,
        };
        preference.remember(latest);
        preference.write().unwrap();
        let saved = Preference::load(path).latest();
        fs::remove_dir_all(dir).unwrap();
        assert_eq!(saved, latest);
    }

    #[test]
    fn keeps_the_size_within_the_display_work_area_and_layout_minimum() {
        let area = Size::from_physical(PhysicalSize::new(1440, 900), 1.5);
        let saved = Size {
            width: 1340,
            height: 100,
        };
        assert_eq!(
            saved.fit(Some(area)),
            Size {
                width: 960,
                height: 250
            }
        );
    }
}
