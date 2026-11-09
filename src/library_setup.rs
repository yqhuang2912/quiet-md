//! The ZIP/portable first launch confirms storage before creating any notes.
use crate::{local_files::Incoming, store::location, theme::Theme, ui};
use gpui::{prelude::FluentBuilder, *};
use std::path::PathBuf;

struct Setup { root: PathBuf, error: String, english: bool, incoming: Option<Incoming> }
impl Setup {
    fn tr(&self, zh: &'static str, en: &'static str) -> &'static str { if self.english { en } else { zh } }
    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, extensions: None,
            prompt: Some(self.tr("选择文档库文件夹", "Choose a library folder").into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await && let Some(root) = paths.into_iter().next() {
                let _ = this.update(cx, |ws, cx| { ws.root = root; ws.error.clear(); cx.notify(); });
            }
        }).detach();
    }
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match location::select_initial(&self.root) {
            Ok(()) => {
                let Some(incoming) = self.incoming.take() else { return };
                let handle = window.window_handle();
                cx.defer(move |cx| {
                    crate::start_library(false, incoming, cx);
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                });
            }
            Err(error) => { self.error = error.to_string(); cx.notify(); }
        }
    }
}
impl Render for Setup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(Theme::bg_body()).text_color(Theme::text()).p(px(32.)).pt(px(48.)).flex().flex_col().gap(px(18.))
            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(self.tr("选择文档库保存位置", "Choose where your notes live")))
            .child(div().text_size(px(13.)).line_height(px(21.)).text_color(Theme::text_caption()).child(self.tr(
                "笔记、图片、附件和 NAS 同步设置将保存在这里。请选择空文件夹，或选择已有的文档库；后续可在设置中更改。",
                "Notes, images, attachments and NAS sync settings will be stored here. Choose an empty folder or an existing library. You can change this later in Settings.")))
            .child(div().flex().items_center().gap(px(12.)).child(div().flex_1().min_w_0().text_size(px(13.)).child(self.root.display().to_string()))
                .child(ui::secondary_button("setup-choose", self.tr("选择文件夹…", "Choose folder…")).on_click(cx.listener(|ws, _, window, cx| ws.choose(window, cx)))))
            .when(!self.error.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(self.error.clone())))
            .child(div().flex_1())
            .child(div().flex().justify_end().gap(px(12.))
                .child(ui::secondary_button("setup-exit", self.tr("退出", "Quit")).on_click(|_, _, cx| cx.quit()))
                .child(ui::primary_button("setup-start", self.tr("使用此位置并开始", "Use this folder and start")).on_click(cx.listener(|ws, _, window, cx| ws.start(window, cx)))))
    }
}
pub fn show(incoming: Incoming, cx: &mut App) {
    crate::settings::startup_language();
    cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(680.), px(380.)), cx))),
        window_min_size: Some(size(px(600.), px(360.))),
        titlebar: Some(TitlebarOptions { title: Some("MyNote".into()), ..Default::default() }),
        app_id: Some(crate::APP_ID.into()), ..Default::default()
    }, |_, cx| cx.new(|_| Setup { root: location::suggested_root(), error: String::new(), english: crate::settings::system_english(), incoming: Some(incoming) }))
        .expect("open library setup");
    cx.activate(true);
}
