//! System tray. Linux: StatusNotifierItem over D-Bus (KDE, GNOME with AppIndicator, most panels).
//! Windows / macOS: tray-icon.

use std::sync::mpsc::Sender;

use crate::i18n::tr;
use crate::{Msg, Waker};

#[derive(Clone, Copy, Debug)]
pub enum TrayCmd {
    Show,
    Toggle,
    Next,
    Prev,
    Quit,
    /// Mouse wheel over the icon: +1 / -1 notch.
    Volume(i32),
}

/// (label, command, freedesktop icon name: drawn by the desktop's own icon theme).
const MENU: [(&str, TrayCmd, &str); 5] = [
    ("Anterior", TrayCmd::Prev, "media-skip-backward"),
    ("Tocar / pausar", TrayCmd::Toggle, "media-playback-start"),
    ("Próxima", TrayCmd::Next, "media-skip-forward"),
    ("Mostrar YtmFast", TrayCmd::Show, "window-new"),
    ("Sair", TrayCmd::Quit, "application-exit"),
];


/// Playback commands run here at once (mpv is thread-safe): the UI thread may be waiting on the
/// compositor for a frame while the window is unfocused or minimised. Set by main.
pub static DIRECT: std::sync::OnceLock<Box<dyn Fn(TrayCmd) -> bool + Send + Sync>> = std::sync::OnceLock::new();

fn send(tx: &Sender<Msg>, waker: &Waker, c: TrayCmd) {
    if !DIRECT.get().is_some_and(|f| f(c)) {
        tx.send(Msg::Tray(c)).ok();
    }
    waker.wake();
}

pub const ICON: usize = 32;

/// The app logo (rendered from packaging/linux/ytmfast.svg), embedded in the binary.
const LOGO_PNG: &[u8] = include_bytes!("../assets/ytmfast-256.png");

/// The logo as straight RGBA, `size` x `size` (256 is the native resolution).
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let Ok(img) = image::load_from_memory(LOGO_PNG) else { return vec![0; (size * size * 4) as usize] };
    let img = if img.width() == size { img } else { img.resize_exact(size, size, image::imageops::FilterType::Lanczos3) };
    img.to_rgba8().into_raw()
}

#[cfg(target_os = "linux")]
mod imp {
    use ksni::TrayMethods;

    use super::*;

    pub struct Tray {
        tx: Sender<Msg>,
        waker: Waker,
        title: String,
        desc: String,
    }

    /// Lets the app refresh the hover tooltip (now playing).
    #[derive(Clone)]
    pub struct TrayHandle(ksni::Handle<Tray>);

    impl TrayHandle {
        pub fn now_playing(&self, rt: &tokio::runtime::Runtime, title: String, desc: String) {
            let h = self.0.clone();
            rt.spawn(async move {
                h.update(move |t| {
                    t.title = title;
                    t.desc = desc;
                })
                .await
            });
        }
    }


    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "ytmfast".into()
        }
        fn title(&self) -> String {
            "YtmFast".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            // Asked again on every tooltip update (each song, each pause): decoded once.
            static ARGB: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
            // SNI wants ARGB.
            let data = ARGB.get_or_init(|| icon_rgba(ICON as u32).chunks(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect()).clone();
            vec![ksni::Icon { width: ICON as i32, height: ICON as i32, data }]
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            send(&self.tx, &self.waker, TrayCmd::Show);
        }
        /// Middle click: play / pause without opening anything.
        fn secondary_activate(&mut self, _x: i32, _y: i32) {
            send(&self.tx, &self.waker, TrayCmd::Toggle);
        }
        fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
            if orientation == ksni::Orientation::Vertical && delta != 0 {
                send(&self.tx, &self.waker, TrayCmd::Volume(-delta.signum()));
            }
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip { title: self.title.clone(), description: self.desc.clone(), ..Default::default() }
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let mut items = vec![];
            for (i, (label, c, icon)) in MENU.into_iter().enumerate() {
                // The three playback buttons, then the window ones.
                if i == 3 {
                    items.push(ksni::MenuItem::Separator);
                }
                items.push(StandardItem { label: tr(label).into(), icon_name: icon.into(), activate: Box::new(move |t: &mut Self| send(&t.tx, &t.waker, c)), ..Default::default() }.into());
            }
            items
        }
    }

    /// None when no tray host is running (the app then just closes normally).
    pub fn spawn(rt: &tokio::runtime::Runtime, tx: Sender<Msg>, waker: Waker) -> Option<TrayHandle> {
        rt.block_on(async move { (Tray { tx, waker, title: "YtmFast".into(), desc: String::new() }).spawn().await.ok().map(TrayHandle) })
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    use super::*;

    #[derive(Clone)]
    pub struct TrayHandle;

    impl TrayHandle {
        pub fn now_playing(&self, _rt: &tokio::runtime::Runtime, _title: String, _desc: String) {}
    }

    /// Must run on the main thread with the event loop alive (eframe's creation context is).
    pub fn spawn(_rt: &tokio::runtime::Runtime, tx: Sender<Msg>, waker: Waker) -> Option<TrayHandle> {
        let menu = Menu::new();
        let items: Vec<(MenuItem, TrayCmd)> = MENU.iter().map(|(label, c, _)| (MenuItem::new(tr(label), true, None), *c)).collect();
        for (i, (item, _)) in items.iter().enumerate() {
            if i == 3 {
                menu.append(&PredefinedMenuItem::separator()).ok();
            }
            menu.append(item).ok();
        }
        let Ok(icon) = Icon::from_rgba(icon_rgba(ICON as u32), ICON as u32, ICON as u32) else { return None };
        let Ok(tray) = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("YtmFast").with_icon(icon).with_menu_on_left_click(false).build() else {
            return None;
        };
        let ids: Vec<_> = items.iter().map(|(item, c)| (item.id().clone(), *c)).collect();
        let (tx2, waker2) = (tx.clone(), waker.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, c)) = ids.iter().find(|(id, _)| *id == e.id) {
                send(&tx2, &waker2, *c);
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                send(&tx, &waker, TrayCmd::Show);
            }
        }));
        // Keep the icon for the whole process.
        std::mem::forget(tray);
        Some(TrayHandle)
    }
}

pub use imp::{TrayHandle, spawn};

/// Tooltip text is a markup subset: escape what could be read as tags.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_logo_decodes() {
        let px = super::icon_rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        // Transparent corner, opaque red body.
        assert_eq!(px[3], 0);
        let mid_left = (16 * 32 + 2) * 4;
        assert!(px[mid_left] > 200 && px[mid_left + 1] < 80 && px[mid_left + 3] > 200);
    }
}
