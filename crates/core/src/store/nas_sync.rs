//! Optional personal NAS sync. Network I/O never holds the document-library lock.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, Component};

const LIMIT: u64 = 128 * 1024 * 1024;
const FILE_LIMIT: u64 = 32 * 1024 * 1024;
type Snapshot = BTreeMap<String, Record>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub url: String,
    pub library: String,
    pub token: String,
}
impl Default for Config {
    fn default() -> Self { Self { enabled: false, url: String::new(), library: "personal".into(), token: String::new() } }
}
impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".nas-sync/config.json");
        if !path.exists() { return Ok(Self::default()); }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        private_dir(root)?;
        write_atomic(&root.join(".nas-sync/config.json"), &serde_json::to_vec_pretty(self)?)
    }
    fn validate(&self) -> Result<()> {
        let url = self.url.trim_end_matches('/');
        anyhow::ensure!(url.starts_with("https://") || url.starts_with("http://"), "服务器地址必须以 http:// 或 https:// 开头");
        anyhow::ensure!(!url.contains(['?', '#', '@']) && !url.chars().any(char::is_whitespace), "服务器地址不能包含账号、查询参数或空格");
        anyhow::ensure!(valid_id(&self.library), "文档库名称只能包含英文字母、数字、下划线和连字符");
        anyhow::ensure!(self.token.len() >= 32 && self.token.bytes().all(|b| b.is_ascii_alphanumeric()), "访问令牌至少需要 32 位英文字母或数字");
        Ok(())
    }
    fn identity(&self) -> String { format!("{}/{}", self.url.trim_end_matches('/'), self.library) }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Record {
    Doc { meta: Value, body: String, deleted: Option<u64>, root: Option<String> },
    Asset { data: String },
}
#[derive(Default, Serialize, Deserialize)]
struct State {
    identity: String,
    device: String,
    server_id: Option<String>,
    versions: BTreeMap<String, u64>,
    baseline: Snapshot,
}
#[derive(Serialize)]
struct Change { base: u64, value: Option<Record> }
#[derive(Deserialize)]
struct Response {
    protocol: u32,
    server_id: String,
    versions: BTreeMap<String, u64>,
    records: Snapshot,
    conflicts: Vec<String>,
}
pub struct Pending {
    _lock: std::fs::File,
    root: PathBuf,
    state: State,
    captured: Snapshot,
    response: Response,
    pub skipped: usize,
}
pub struct Report { pub changed: usize, pub conflicts: usize, pub skipped: usize }

fn valid_id(id: &str) -> bool { !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') }
fn valid_asset(name: &str) -> bool {
    !name.is_empty() && name.len() <= 1024 && name.split('/').all(|part| {
        let stem = part.split('.').next().unwrap_or_default().to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") ||
            (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && matches!(stem.as_bytes()[3], b'1'..=b'9'));
        !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' ']) && !reserved &&
            !part.chars().any(|c| c.is_control() || "<>:\"|?*\\".contains(c))
    })
}
fn safe_path(root: &Path, relative: &str) -> Result<PathBuf> {
    anyhow::ensure!(Path::new(relative).components().all(|c| matches!(c, Component::Normal(_))), "同步路径越界");
    let canonical = root.canonicalize()?;
    let mut path = root.to_path_buf();
    for part in Path::new(relative).components() {
        path.push(part);
        if path.try_exists()? { anyhow::ensure!(path.canonicalize()?.starts_with(&canonical), "同步路径越过符号链接"); }
        else if let Ok(md) = path.symlink_metadata() { anyhow::ensure!(!md.file_type().is_symlink(), "同步路径包含无效符号链接"); }
    }
    Ok(path)
}
fn private_dir(root: &Path) -> Result<()> {
    let path = safe_path(root, ".nas-sync")?;
    std::fs::create_dir_all(&path)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn portable_meta(meta: &Meta) -> Result<Value> {
    let mut meta = meta.clone();
    meta.opened = 0;
    meta.catalogue_closed = true;
    meta.width = PageWidth::default();
    meta.reading = false;
    meta.info = InfoRow::default();
    meta.refs_hidden = false;
    Ok(serde_json::to_value(meta)?)
}

impl Store {
    fn sync_snapshot(&self) -> Result<Snapshot> {
        let mut result = Snapshot::new();
        for (id, meta, deleted, root) in self.index.iter().map(|(id, m)| (id, m, None, None))
            .chain(self.trash.iter().filter(|(id, _)| !self.index.contains_key(*id)).map(|(id, t)| (id, &t.meta, Some(t.deleted), t.root.clone()))) {
            if meta.local() { continue; }
            anyhow::ensure!(valid_id(id), "文档 ID 不适用于跨平台同步：{id}");
            let body = if meta.folder.is_some() { String::new() } else {
                let path = format!("{}/{}.{}", if deleted.is_some() { "trash" } else { "docs" }, id, if meta.markdown { "md" } else { "xml" });
                String::from_utf8(read_bounded(&safe_path(&self.root, &path)?, FILE_LIMIT)?)?
            };
            result.insert(format!("doc/{id}"), Record::Doc { meta: portable_meta(meta)?, body, deleted, root });
        }
        fn assets(root: &Path, dir: &Path, result: &mut Snapshot, total: &mut u64) -> Result<()> {
            if !dir.exists() { return Ok(()); }
            for entry in std::fs::read_dir(dir)? {
                let path = entry?.path();
                let relative = path.strip_prefix(root.join("assets"))?.to_str().context("附件路径不是 UTF-8")?.replace('\\', "/");
                if matches!(relative.split('/').next(), Some("thumbs" | "remote")) { continue; }
                anyhow::ensure!(valid_asset(&relative), "附件名不适用于跨平台同步：{relative}");
                let path = safe_path(root, &format!("assets/{relative}"))?;
                anyhow::ensure!(!path.symlink_metadata()?.file_type().is_symlink(), "同步附件不能是符号链接");
                if path.is_dir() { assets(root, &path, result, total)?; }
                else if path.is_file() {
                    let data = read_bounded(&path, FILE_LIMIT)?;
                    *total += data.len() as u64;
                    anyhow::ensure!(*total <= LIMIT / 2, "附件总量超过首版 64 MiB 限制");
                    result.insert(format!("asset/{relative}"), Record::Asset { data: STANDARD.encode(data) });
                }
            }
            Ok(())
        }
        assets(&self.root, &self.root.join("assets"), &mut result, &mut 0)?;
        Ok(result)
    }
}

/// Run this on a worker thread. `apply` is a separate, short local transaction
/// so the UI can flush every open editor after the network response arrives.
pub fn prepare(root: PathBuf, config: Config) -> Result<Pending> {
    config.validate()?;
    private_dir(&root)?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(root.join(".nas-sync/session.lock"))?;
    lock.try_lock().context("另一个窗口或进程正在同步")?;
    let path = root.join(".nas-sync/state.json");
    let mut state: State = if path.exists() { serde_json::from_slice(&read_bounded(&path, LIMIT * 2)?)
        .context("同步状态损坏，请先保留 .nas-sync/state.json 排查，不能直接覆盖")? } else { State::default() };
    if state.identity != config.identity() {
        state = State { identity: config.identity(), device: uuid::Uuid::new_v4().to_string(), ..Default::default() };
        // Keep the same device ID when the first response is lost in transit.
        write_atomic(&path, &serde_json::to_vec(&state)?)?;
    }
    anyhow::ensure!(valid_id(&state.device), "同步设备标识无效");
    let mut store = Store::open_at(root.clone())?;
    anyhow::ensure!(store.warnings.is_empty(), "文档库需要先处理恢复提示后再同步");
    store.list();
    let guard = store.begin_write()?;
    let captured = store.sync_snapshot()?;
    let skipped = store.index.values().filter(|m| m.local()).count();
    drop(guard);
    let mut changes = BTreeMap::new();
    let keys: BTreeSet<_> = captured.keys().chain(state.baseline.keys()).cloned().collect();
    for key in keys {
        if key.starts_with("asset/") && !captured.contains_key(&key) { continue; }
        if captured.get(&key) != state.baseline.get(&key) {
            changes.insert(key.clone(), Change { base: state.versions.get(&key).copied().unwrap_or(0), value: captured.get(&key).cloned() });
        }
    }
    let body = serde_json::to_vec(&serde_json::json!({ "protocol": 1, "library": config.library, "device": state.device,
        "server_id": state.server_id, "changes": changes }))?;
    anyhow::ensure!(body.len() as u64 <= LIMIT, "文档库超过首版 128 MiB 传输限制");
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(60)).redirects(0).build();
    let response = agent.post(&format!("{}/v1/sync", config.url.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {}", config.token)).set("Content-Type", "application/json").send_bytes(&body)
        .map_err(|error| match error {
            ureq::Error::Status(code, response) => {
                let message = response.into_string().unwrap_or_default();
                anyhow::anyhow!("NAS 返回 {code}：{}", message.chars().take(500).collect::<String>())
            }
            ureq::Error::Transport(_) => anyhow::anyhow!("无法连接 NAS；请检查服务器地址、Tailscale 连接和访问规则"),
        })?;
    let mut bytes = vec![];
    response.into_reader().take(LIMIT + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= LIMIT, "NAS 返回数据超过大小限制");
    let response: Response = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(response.protocol == 1 && !response.server_id.is_empty(), "不兼容的同步服务");
    anyhow::ensure!(state.server_id.as_ref().is_none_or(|s| s == &response.server_id), "NAS 数据库标识发生变化，请先恢复 NAS 备份");
    for (key, value) in &response.records { validate_record(key, value)?; }
    for (key, revision) in &response.versions {
        validate_key(key)?;
        anyhow::ensure!(*revision > 0, "NAS 返回了无效修订号");
    }
    anyhow::ensure!(response.records.keys().all(|key| response.versions.contains_key(key)) && changes.keys().all(|key| response.versions.contains_key(key)), "NAS 返回的同步快照不完整");
    // Persist the identity even if the app exits before applying the response.
    state.server_id = Some(response.server_id.clone());
    write_atomic(&path, &serde_json::to_vec(&state)?)?;
    Ok(Pending { _lock: lock, root, state, captured, response, skipped })
}

fn validate_key(key: &str) -> Result<()> {
    let valid = key.strip_prefix("doc/").is_some_and(valid_id) || key.strip_prefix("asset/").is_some_and(valid_asset);
    anyhow::ensure!(valid, "NAS 返回了无效记录路径");
    Ok(())
}
fn validate_record(key: &str, value: &Record) -> Result<()> {
    validate_key(key)?;
    match value {
        Record::Doc { meta, body, root, .. } => {
            anyhow::ensure!(key.starts_with("doc/"), "文档类型不匹配");
            let meta: Meta = serde_json::from_value(meta.clone())?;
            anyhow::ensure!(!meta.local() && root.as_deref().is_none_or(valid_id) && meta.parent.as_deref().is_none_or(valid_id), "NAS 文档包含无效路径或引用");
            anyhow::ensure!(body.len() as u64 <= FILE_LIMIT, "NAS 文档过大");
            if meta.folder.is_none() && !meta.markdown { docxml::parse(body)?; }
        }
        Record::Asset { data } => {
            anyhow::ensure!(key.starts_with("asset/") && STANDARD.decode(data)?.len() as u64 <= FILE_LIMIT, "NAS 附件无效或过大");
        }
    }
    Ok(())
}

impl Pending {
    pub fn apply(mut self, store: &mut Store) -> Result<Report> {
        anyhow::ensure!(self.root.canonicalize()? == store.root.canonicalize()?, "同步结果属于其他文档库");
        let _guard = store.begin_write()?;
        let current = store.sync_snapshot()?;
        let mut next = current.clone();
        for key in self.response.versions.keys() {
            let edited = current.get(key) != self.captured.get(key);
            let received = self.response.records.get(key);
            // Keep edits made during I/O, but acknowledge an accepted upload.
            // An unseen remote edit must still conflict on the next exchange.
            if edited && received != self.captured.get(key) { continue; }
            match received {
                Some(record) => { if !edited { next.insert(key.clone(), record.clone()); } self.state.baseline.insert(key.clone(), record.clone()); }
                None => { if !edited { next.remove(key); } self.state.baseline.remove(key); }
            }
            self.state.versions.insert(key.clone(), self.response.versions[key]);
        }
        let changed: Vec<_> = current.keys().chain(next.keys()).collect::<BTreeSet<_>>().into_iter().filter(|key| current.get(*key) != next.get(*key)).cloned().collect();
        // Keep the pre-apply documents before the first disk mutation. These
        // backups also cover an interrupted index/body move between directories.
        let recovery: Snapshot = changed.iter().filter(|key| key.starts_with("doc/")).filter_map(|key| current.get(key).map(|v| (key.clone(), v.clone()))).collect();
        if !recovery.is_empty() {
            let dir = safe_path(&store.root, ".nas-sync/recovery")?;
            std::fs::create_dir_all(&dir)?;
            write_atomic(&dir.join(format!("{}.json", uuid::Uuid::new_v4())), &serde_json::to_vec(&recovery)?)?;
        }
        let mut obsolete = vec![];
        for key in &changed {
            if let Some(id) = key.strip_prefix("doc/") {
                anyhow::ensure!(!store.index.get(id).is_some_and(Meta::local), "远端 ID 与本机外部文件冲突");
            } else if let Some(name) = key.strip_prefix("asset/") && let Some(Record::Asset { data }) = next.get(key) {
                let path = safe_path(&store.root, &format!("assets/{name}"))?;
                if path.exists() { anyhow::ensure!(read_bounded(&path, FILE_LIMIT)? == STANDARD.decode(data)?, "本机附件同名冲突，已保留原文件"); }
            }
        }
        for key in &changed {
            if let Some(id) = key.strip_prefix("doc/") {
                let local_meta = store.index.get(id).cloned().or_else(|| store.trash.get(id).map(|t| t.meta.clone()));
                if let Some(old) = current.get(key).and_then(|v| doc_file(id, v)) { obsolete.push(old); }
                store.index.remove(id);
                store.trash.remove(id);
                if let Some(Record::Doc { meta, body, deleted, root }) = next.get(key) {
                    let mut meta: Meta = serde_json::from_value(meta.clone())?;
                    if let Some(local) = local_meta {
                        meta.opened = local.opened;
                        meta.catalogue_closed = local.catalogue_closed;
                        meta.width = local.width;
                        meta.reading = local.reading;
                        meta.info = local.info;
                        meta.refs_hidden = local.refs_hidden;
                    }
                    if let Some(file) = doc_file(id, &next[key]) {
                        let path = safe_path(&store.root, &file)?;
                        std::fs::create_dir_all(path.parent().unwrap())?;
                        write_atomic(&path, body.as_bytes())?;
                        obsolete.retain(|old| old != &file);
                    }
                    if let Some(deleted) = deleted { store.trash.insert(id.into(), Trashed { meta, deleted: *deleted, root: root.clone() }); }
                    else { store.index.insert(id.into(), meta); }
                }
            } else if let Some(name) = key.strip_prefix("asset/") && let Some(Record::Asset { data }) = next.get(key) {
                let path = safe_path(&store.root, &format!("assets/{name}"))?;
                let bytes = STANDARD.decode(data)?;
                if path.exists() { anyhow::ensure!(read_bounded(&path, FILE_LIMIT)? == bytes, "本机附件同名冲突，已保留原文件"); }
                else { std::fs::create_dir_all(path.parent().unwrap())?; write_atomic(&path, &bytes)?; }
            }
        }
        if store.index.values().chain(store.trash.values().map(|t| &t.meta)).any(|m| m.markdown) { store.require_markdown_format()?; }
        store.save_index()?;
        store.save_trash()?;
        for file in obsolete { let path = safe_path(&store.root, &file)?; if path.exists() { std::fs::remove_file(path)?; } }
        store.parsed.clear();
        write_atomic(&store.root.join(".nas-sync/state.json"), &serde_json::to_vec(&self.state)?)?;
        Ok(Report { changed: changed.len(), conflicts: self.response.conflicts.len(), skipped: self.skipped })
    }
}
fn doc_file(id: &str, record: &Record) -> Option<String> {
    let Record::Doc { meta, deleted, .. } = record else { return None };
    if meta.get("folder").is_some_and(|v| !v.is_null()) { return None; }
    Some(format!("{}/{}.{}", if deleted.is_some() { "trash" } else { "docs" }, id, if meta.get("markdown").and_then(Value::as_bool).unwrap_or(false) { "md" } else { "xml" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Library(PathBuf);
    impl Library {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("mynote-sync-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn open(&self) -> Store { Store::open_at(self.0.clone()).unwrap() }
    }
    impl Drop for Library { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    fn record(title: &str) -> Record {
        let mut doc = Document::default();
        doc.title = crate::model::RichText::plain(title);
        Record::Doc { meta: portable_meta(&Meta::default()).unwrap(), body: docxml::serialize(&doc), deleted: None, root: None }
    }
    fn pending(store: &Store, captured: Snapshot, remote: Snapshot) -> Pending {
        private_dir(&store.root).unwrap();
        let versions = captured.keys().map(|key| (key.clone(), 1)).collect();
        let remote_versions = captured.keys().chain(remote.keys()).map(|key| (key.clone(), 2)).collect();
        Pending { _lock: std::fs::File::create(store.root.join(".nas-sync/test.lock")).unwrap(), root: store.root.clone(),
            state: State { identity: "test".into(), device: "a".into(), server_id: Some("server".into()), baseline: captured.clone(), versions },
            captured, response: Response { protocol: 1, server_id: "server".into(), records: remote, versions: remote_versions, conflicts: vec![] }, skipped: 0 }
    }
    #[test]
    fn incoming_documents_assets_and_trash_are_portable() {
        let lib = Library::new();
        let mut store = lib.open();
        let mut remote = Snapshot::new();
        remote.insert("doc/note".into(), record("from Mac"));
        remote.insert("asset/files/example/图片.png".into(), Record::Asset { data: STANDARD.encode(b"image-content") });
        pending(&store, Snapshot::new(), remote.clone()).apply(&mut store).unwrap();
        assert_eq!(serde_json::from_slice::<FormatMarker>(&std::fs::read(lib.0.join("format.json")).unwrap()).unwrap().format, BASE_FORMAT);
        assert_eq!(store.load("note").unwrap().title.text, "from Mac");
        assert_eq!(std::fs::read(lib.0.join("assets/files/example/图片.png")).unwrap(), b"image-content");
        let captured = store.sync_snapshot().unwrap();
        if let Record::Doc { deleted, .. } = remote.get_mut("doc/note").unwrap() { *deleted = Some(crate::time::now()); }
        pending(&store, captured, remote).apply(&mut store).unwrap();
        assert!(!lib.0.join("docs/note.xml").exists());
        assert!(lib.0.join("trash/note.xml").exists());
        let mut reopened = lib.open();
        assert!(reopened.list().is_empty());
        assert_eq!(reopened.list_trash().len(), 1);
    }
    #[test]
    fn local_edit_during_request_is_not_overwritten_or_acknowledged() {
        let lib = Library::new();
        let mut store = lib.open();
        let (id, mut doc) = store.create().unwrap();
        let captured = store.sync_snapshot().unwrap();
        let mut remote = captured.clone();
        remote.insert(format!("doc/{id}"), record("remote change"));
        let exchange = pending(&store, captured.clone(), remote);
        doc.title = crate::model::RichText::plain("typed while syncing");
        store.save(&id, &doc).unwrap();
        exchange.apply(&mut store).unwrap();
        assert_eq!(store.load(&id).unwrap().title.text, "typed while syncing");
        let state: State = serde_json::from_slice(&std::fs::read(lib.0.join(".nas-sync/state.json")).unwrap()).unwrap();
        assert_eq!(state.versions[&format!("doc/{id}")], 1);
        assert_eq!(state.baseline, captured);
    }
    #[test]
    fn remote_purge_retains_recovery_and_local_preferences_do_not_roam() {
        let lib = Library::new();
        let mut store = lib.open();
        let (id, _) = store.create().unwrap();
        store.update_meta(&id, |m| { m.opened = 123; m.width = PageWidth::Wide; m.reading = true; }).unwrap();
        let captured = store.sync_snapshot().unwrap();
        let mut remote = captured.clone();
        remote.insert(format!("doc/{id}"), record("new text"));
        pending(&store, captured, remote).apply(&mut store).unwrap();
        assert_eq!(store.index[&id].opened, 123);
        assert_eq!(store.index[&id].width, PageWidth::Wide);
        assert!(store.index[&id].reading);
        let captured = store.sync_snapshot().unwrap();
        pending(&store, captured, Snapshot::new()).apply(&mut store).unwrap();
        assert!(!lib.0.join(format!("docs/{id}.xml")).exists());
        assert!(std::fs::read_dir(lib.0.join(".nas-sync/recovery")).unwrap().count() >= 2);
    }
    #[test]
    fn asset_collision_is_checked_before_changing_documents() {
        let lib = Library::new();
        let mut store = lib.open();
        let (id, _) = store.create().unwrap();
        std::fs::create_dir_all(lib.0.join("assets")).unwrap();
        std::fs::write(lib.0.join("assets/image.png"), b"original").unwrap();
        let captured = store.sync_snapshot().unwrap();
        let mut remote = captured.clone();
        remote.insert(format!("doc/{id}"), record("should not be applied"));
        remote.insert("asset/image.png".into(), Record::Asset { data: STANDARD.encode(b"collision") });
        assert!(pending(&store, captured.clone(), remote).apply(&mut store).is_err());
        assert_eq!(store.sync_snapshot().unwrap(), captured);
    }
    #[test]
    fn invalid_paths_and_external_documents_are_rejected() {
        for key in ["asset/../secret", "asset/C:/secret", "asset/CON.txt", "asset/a\\b", "doc/../secret"] { assert!(validate_key(key).is_err(), "{key}"); }
        let mut value = record("external");
        if let Record::Doc { meta, .. } = &mut value { meta["path"] = Value::String("/Users/me/file.md".into()); }
        assert!(validate_record("doc/note", &value).is_err());
        let lib = Library::new();
        assert!(safe_path(&lib.0, "../outside").is_err());
    }
    #[test]
    fn incoming_markdown_marks_format_before_writing_the_new_index() {
        let lib = Library::new();
        let mut store = lib.open();
        let mut remote = Snapshot::new();
        let mut meta = Meta::default();
        meta.markdown = true;
        meta.name = Some("Markdown from Mac".into());
        remote.insert("doc/markdown".into(), Record::Doc { meta: portable_meta(&meta).unwrap(), body: "# Body\n".into(), deleted: None, root: None });
        pending(&store, Snapshot::new(), remote).apply(&mut store).unwrap();
        assert_eq!(std::fs::read_to_string(lib.0.join("docs/markdown.md")).unwrap(), "# Body\n");
        assert_eq!(serde_json::from_slice::<FormatMarker>(&std::fs::read(lib.0.join("format.json")).unwrap()).unwrap().format, FORMAT);
        assert!(lib.open().is_markdown("markdown"));
    }
}
