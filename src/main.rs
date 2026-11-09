#![recursion_limit = "512"]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

pub(crate) const APP_ID: &str = "io.github.yqhuang2912.mynote";

mod app;
mod shared_store;
mod quick_note;
mod assets;
mod changelog;
mod guides;
mod clipboard;
#[cfg(feature = "devtools")]
mod devtools;
/// Without the `devtools` feature (release builds) nothing reads FDL_SCRIPT.
#[cfg(not(feature = "devtools"))]
mod devtools {
    pub static RENDER_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    pub fn scripted() -> bool { false }
    pub fn start(_: &mut gpui::Window, _: &mut gpui::App) {}
    pub fn take_pick() -> Option<Vec<std::path::PathBuf>> { None }
    pub fn file_drag() -> bool { false }
}
mod docinfo;
mod docmenu;
mod editor;
mod highlight;
mod home;
mod html;
mod input;
mod logging;
mod modes;
#[cfg(target_os = "macos")]
mod native_header;
#[cfg(not(target_os = "macos"))]
#[path = "native_header_fallback.rs"]
mod native_header;
mod printing;
mod search;
mod sidebar;
mod local_files;
mod sidebar_drag;
mod home_folder;
mod folders;
mod file_history;
mod settings;
mod themes;
mod media_theme;
mod shortcuts;
#[allow(dead_code, unused_imports)]
mod feishu_theme;
#[allow(dead_code, unused_imports)]
mod editor_theme;
#[allow(dead_code, unused_imports)]
mod editor_ui;
mod theme;
mod transfer;
mod trash;
mod tree;
mod ui;
mod update;
mod nas_sync;
mod library_setup;

use fdl_core::{docxml, lang, markdown, model, palette, paste, store, time};

use gpui::*;

#[cfg(target_os = "macos")]
fn set_app_icon() {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;
    let png = include_bytes!("../assets/branding/app-icon.png");
    unsafe {
        let data: *mut Object = msg_send![class!(NSData), dataWithBytes:png.as_ptr() length:png.len()];
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData:data];
        if !image.is_null() {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, setApplicationIconImage:image];
            let _: () = msg_send![image, release];
        }
    }
}

/// The main window, centred at the remembered size.
fn open_main_window(scripted: bool, cx: &mut App) {
    // Scripted dev runs must not steal keyboard focus from whatever the
    // user is typing into.
    let bounds = Bounds::centered(None, quick_note::main_window_size(cx), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: crate::theme::window_background(),
            app_id: Some(crate::APP_ID.into()),
            window_decorations: if cfg!(target_os = "linux") { Some(WindowDecorations::Client) } else { None },
            titlebar: Some(TitlebarOptions {
                title: Some("Quiet.md".into()),
                appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
                traffic_light_position: Some(point(px(20.), px(app::HEADER_H / 2. - 7.))),
            }),
            window_min_size: Some(size(px(480.), px(480.))),
            focus: !scripted,
            ..Default::default()
        },
        |window, cx| {
            devtools::start(window, cx);
            cx.new(|cx| app::Workspace::new(window, cx))
        },
    )
    .expect("open window");
}

fn main() {
    logging::init();
    let scripted = devtools::scripted();
    let app = Application::new().with_assets(assets::Assets);
    let incoming = local_files::Incoming::from_args();
    let urls = incoming.clone();
    app.on_open_urls(move |u| urls.push_urls(u));
    app.on_reopen(|cx| if cx.has_global::<shared_store::SharedStore>() && cx.windows().is_empty() { open_main_window(false, cx); });
    app.run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        set_app_icon();
        match store::location::needs_setup() {
            Ok(true) if !scripted => library_setup::show(incoming, cx),
            Ok(_) => start_library(scripted, incoming, cx),
            Err(error) => logging::fatal("无法打开文档库位置设置 / Could not open library location", &error),
        }
    });
}

fn start_library(scripted: bool, incoming: local_files::Incoming, cx: &mut App) {
    let location_warning = match store::location::initialize() {
        Ok(warning) => warning,
        Err(error) => logging::fatal("无法打开文档库位置设置 / Could not open library location", &error),
    };
    // No single-instance guard: copies may share a library (builds tested
    // side by side). `Store` rereads an index another copy rewrote before
    // writing its own, and `guides` seeds a new library only once.
    let root = store::Store::root_dir();
    logging::write(&format!("library {}", root.display()));
    // A library that was here before this launch can show 更新日志.
    let existed = guides::seeded(&root).unwrap_or(false);
    if !scripted && let Err(err) = guides::initialize(&root) {
        logging::fatal(&format!("无法初始化文档库 / Could not set up the library at {}", root.display()), &err);
    }
    if !scripted && let Err(error) = store::location::finish_setup() {
        logging::fatal("无法保存文档库位置 / Could not save library location", &error);
    }
    editor::math::load_fonts(cx);
    editor::image::init_pictures(cx);
    input::bind_keys(cx);
    editor::keymap::bind_keys(cx);
    editor::image::bind_keys(cx);
    app::bind_keys(cx);
    settings::load();
    settings::apply_appearance(&settings::prefs().appearance);
    settings::sync_dark(cx);
    themes::load(&settings::prefs().theme_id(), false, || cx.text_system().all_font_names());
    changelog::startup(existed);
    shortcuts::init(cx);
    match shared_store::SharedStore::open() {
        Ok(s) => cx.set_global(s),
        Err(err) => logging::fatal("无法打开文档库 / Could not open the library", &err),
    }
    let quick_enabled = !scripted;
    if quick_enabled { quick_note::install(cx); }
    app::set_menus(cx);
    update::init(cx);
    nas_sync::init(cx);
    open_main_window(scripted, cx);
    if let Some(message) = location_warning {
        logging::write(&message);
        if let Some(window) = cx.windows().first().and_then(|w| w.downcast::<app::Workspace>()) {
            let _ = window.update(cx, |ws, _, cx| ws.show_toast_kind(ui::ToastKind::Error, &message, cx));
        }
    }
    incoming.deliver(cx);
    cx.on_window_closed(move |cx| {
        if (!quick_enabled || !cfg!(target_os = "macos")) && cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    if !scripted {
        cx.activate(true);
    }
}
