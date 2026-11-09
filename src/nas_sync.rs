//! Desktop scheduling and settings for the optional NAS service.
use gpui::*;
use crate::{app::Workspace, input::TextField, settings::tr, shared_store::SharedStore, store::{Store, nas_sync::{self, Config}}};
use std::time::Duration;

pub struct Form { pub url: Entity<TextField>, pub library: Entity<TextField>, pub token: Entity<TextField> }
impl Form {
    pub fn new(cx: &mut Context<Workspace>) -> Self {
        let config = cx.try_global::<Client>().map(|c| c.config.clone()).unwrap_or_default();
        let url = cx.new(|cx| { let mut field = TextField::new("https://nas.example.ts.net", cx); field.set_text(&config.url, cx); field });
        let library = cx.new(|cx| { let mut field = TextField::new("personal", cx); field.set_text(&config.library, cx); field });
        let token = cx.new(|cx| { let mut field = TextField::new(tr("粘贴 NAS 访问令牌", "Paste your NAS access token"), cx); field.max_len = Some(256); field });
        Self { url, library, token }
    }
}
pub struct Client { pub config: Config, pub busy: bool, pub status: String }
impl Global for Client {}

pub fn init(cx: &mut App) {
    let (config, status) = match Config::load(&Store::root_dir()) {
        Ok(config) => (config, tr("尚未同步", "Not synced yet").into()),
        Err(error) => (Config::default(), format!("{}: {error}", tr("无法读取同步设置", "Could not read sync settings"))),
    };
    cx.set_global(Client { config, busy: false, status });
    cx.spawn(async |cx| {
        cx.background_executor().timer(Duration::from_secs(5)).await;
        loop {
            if cx.update(|cx| { if cx.global::<Client>().config.enabled { start(cx); } }).is_err() { break; }
            cx.background_executor().timer(Duration::from_secs(30)).await;
        }
    }).detach();
}
fn status(message: String, cx: &mut App) {
    cx.global_mut::<Client>().status = message;
    cx.refresh_windows();
}
pub fn toggle(cx: &mut App) {
    if cx.global::<Client>().busy { return; }
    let mut config = cx.global::<Client>().config.clone();
    config.enabled = !config.enabled;
    match config.save(&Store::root_dir()) {
        Ok(()) => {
            let enabled = config.enabled;
            cx.global_mut::<Client>().config = config;
            status(tr("自动同步设置已保存", "Automatic sync setting saved").into(), cx);
            if enabled { start(cx); }
        }
        Err(error) => status(error.to_string(), cx),
    }
}
impl Workspace {
    pub fn save_nas_config(&mut self, cx: &mut Context<Self>) {
        if cx.global::<Client>().busy { return; }
        let entered = self.nas_form.token.read(cx).text.trim().to_string();
        let config = Config {
            enabled: true,
            url: self.nas_form.url.read(cx).text.trim().trim_end_matches('/').into(),
            library: self.nas_form.library.read(cx).text.trim().into(),
            token: if entered.is_empty() { cx.global::<Client>().config.token.clone() } else { entered },
        };
        match config.save(&Store::root_dir()) {
            Ok(()) => {
                self.nas_form.token.update(cx, |field, cx| field.set_text("", cx));
                cx.global_mut::<Client>().config = config;
                cx.defer(start);
            }
            Err(error) => status(error.to_string(), cx),
        }
    }
}
fn flush_windows(cx: &mut App) -> bool {
    cx.windows().into_iter().filter_map(|w| w.downcast::<Workspace>())
        .map(|w| w.update(cx, |ws, _, cx| ws.flush(cx)).unwrap_or(false)).fold(true, |ready, saved| ready & saved)
}
pub fn start(cx: &mut App) {
    if cx.global::<Client>().busy || !cx.has_global::<SharedStore>() { return; }
    if !flush_windows(cx) { status(tr("请先处理未保存的文档，再同步", "Resolve unsaved documents before syncing").into(), cx); return; }
    let config = cx.global::<Client>().config.clone();
    cx.global_mut::<Client>().busy = true;
    status(tr("正在同步…", "Syncing…").into(), cx);
    let task = cx.background_executor().spawn(async move { nas_sync::prepare(Store::root_dir(), config) });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = cx.update(|cx| {
            let result = result.and_then(|pending| {
                anyhow::ensure!(flush_windows(cx), "请先处理未保存的文档，再同步");
                cx.global::<SharedStore>().clone().apply_nas_sync(pending)
            });
            cx.global_mut::<Client>().busy = false;
            match result {
                Ok(report) => {
                    for window in cx.windows().into_iter().filter_map(|w| w.downcast::<Workspace>()) {
                        let _ = window.update(cx, |ws, _, cx| ws.refresh_after_nas_sync(cx));
                    }
                    let now = crate::time::now();
                    let (_, _, _, hour, minute) = crate::time::civil(now);
                    let time = format!("{hour:02}:{minute:02}");
                    status(if crate::settings::english() { format!("Synced {time} · {} changes · {} conflicts · {} external files skipped", report.changed, report.conflicts, report.skipped) }
                        else { format!("{time} 已同步 · {} 项变更 · {} 个冲突 · 跳过 {} 个外部文件", report.changed, report.conflicts, report.skipped) }, cx);
                }
                Err(error) => status(format!("{}：{error:#}", tr("同步未完成，下次重试", "Sync incomplete; will retry")), cx),
            }
        });
    }).detach();
}
