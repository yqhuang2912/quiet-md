//! Device-local library location. A pending copy runs before the library opens;
//! the active location only changes after every file has reached its destination.
use super::write_atomic;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, sync::OnceLock};

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static ALIASES: OnceLock<Vec<PathBuf>> = OnceLock::new();

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Location { root: Option<PathBuf>, pending: Option<Migration>, aliases: Vec<PathBuf>, confirmed: bool, create: bool }
#[derive(Clone, Serialize, Deserialize)]
struct Migration { source: PathBuf, target: PathBuf }

fn config_path() -> PathBuf {
    // Separate from preferences.json: we need this before opening any library.
    std::env::var_os("QUIET_MD_CONFIG_HOME").map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("io.github.yqhuang2912.mynote"))
        .join("library.json")
}
fn default_root() -> PathBuf {
    dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents"))).unwrap_or_else(|| PathBuf::from(".")).join("Quiet.md")
}
pub fn suggested_root() -> PathBuf { read(&config_path()).ok().and_then(|c| c.root).unwrap_or_else(default_root) }
pub fn needs_setup() -> Result<bool> { Ok(!overridden() && !read(&config_path())?.confirmed) }
fn validate_initial(root: &Path) -> Result<()> {
    ensure!(root.is_absolute() && root.parent().is_some(), "请选择文档库文件夹，不能使用磁盘根目录 / Choose a library folder, not a drive root");
    if root.exists() {
        ensure!(root.is_dir(), "请选择文件夹 / Select a folder");
        ensure!(fs::read_dir(root)?.next().is_none() || (root.join("docs").is_dir() && root.join("format.json").is_file()), "请选择空文件夹或已有的文档库 / Choose an empty folder or an existing document library");
    }
    Ok(())
}
fn select_initial_at(path: &Path, root: &Path) -> Result<()> {
    ensure!(!read(path)?.confirmed, "文档库位置已确认，请在设置中更改 / Change the confirmed location in Settings");
    ensure!(!path.starts_with(root), "文档库不能包含应用的位置设置文件 / Choose a library outside the application's location settings folder");
    validate_initial(root)?;
    save(path, &Location { root: Some(root.to_path_buf()), confirmed: true, create: true, ..Default::default() })
}
pub fn select_initial(root: &Path) -> Result<()> { select_initial_at(&config_path(), root) }
/// The empty-folder exception is only for setup, never normal later launches.
pub fn finish_setup() -> Result<()> {
    if overridden() { return Ok(()); }
    let path = config_path();
    let mut config = read(&path)?;
    if config.create {
        let root = config.root.as_ref().context("文档库位置缺失 / Library location missing")?;
        ensure!(root.join("docs").is_dir() && root.join("format.json").is_file(), "文档库初始化未完成 / Library initialization incomplete");
        config.create = false;
        save(&path, &config)?;
    }
    Ok(())
}
fn read(path: &Path) -> Result<Location> {
    match fs::read(path) {
        Ok(bytes) => {
            let config: Location = serde_json::from_slice(&bytes).context("无法读取文档库位置设置 / Invalid library location settings")?;
            ensure!(config.root.as_ref().is_none_or(|p| p.is_absolute()), "文档库位置必须是绝对路径 / Library path must be absolute");
            Ok(config)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Location::default()),
        Err(e) => Err(e.into()),
    }
}
fn save(path: &Path, config: &Location) -> Result<()> {
    fs::create_dir_all(path.parent().context("位置设置没有父目录")?)?;
    write_atomic(path, &serde_json::to_vec_pretty(config)?)
}

pub fn overridden() -> bool { std::env::var_os("QUIET_MD_HOME").is_some() }
pub fn root_dir() -> PathBuf {
    ROOT.get_or_init(|| std::env::var_os("QUIET_MD_HOME").map(PathBuf::from)
        .unwrap_or_else(|| read(&config_path()).ok().and_then(|c| c.root).unwrap_or_else(default_root))).clone()
}

fn plain(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(path) = path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        return PathBuf::from(path.strip_prefix("UNC\\").map(|p| format!(r"\\{p}")).unwrap_or_else(|| path.into()));
    }
    path.to_path_buf()
}
fn rebase(path: &Path, aliases: &[PathBuf], root: &Path) -> PathBuf {
    let path = plain(path);
    for old in aliases {
        if let Ok(relative) = path.strip_prefix(plain(old)) { return root.join(relative); }
    }
    path
}
/// Old absolute image/attachment paths follow the copy even after its backup
/// is removed. Relative references and files outside the old library stay as-is.
pub fn resolve(path: &Path) -> PathBuf {
    if overridden() { return path.to_path_buf(); }
    let aliases = ALIASES.get_or_init(|| read(&config_path()).map(|c| c.aliases).unwrap_or_default());
    rebase(path, aliases, &root_dir())
}

/// A failed migration keeps using the original library and reports the error.
/// An unavailable saved library is an error, never a silently created empty one.
pub fn initialize() -> Result<Option<String>> {
    if let Some(root) = std::env::var_os("QUIET_MD_HOME") { let _ = ROOT.set(root.into()); return Ok(None); }
    let (root, warning) = initialize_at(&config_path(), default_root())?;
    let _ = ALIASES.set(read(&config_path())?.aliases);
    let _ = ROOT.set(root);
    Ok(warning)
}
fn initialize_at(path: &Path, fallback: PathBuf) -> Result<(PathBuf, Option<String>)> {
    let mut config = read(path)?;
    let mut root = config.root.clone().unwrap_or(fallback);
    if config.create {
        validate_initial(&root)?;
        fs::create_dir_all(&root)?;
    } else if config.root.is_some() { ensure!(root.join("docs").is_dir() && root.join("format.json").is_file(), "文档库不可用，请连接磁盘后重试 / Library unavailable; reconnect its disk: {}", root.display()); }
    let mut warning = None;
    if let Some(migration) = config.pending.clone() {
        let result = (|| -> Result<()> {
            ensure!(root.canonicalize()? == migration.source.canonicalize()?, "待迁移的文档库已变更 / Migration source changed");
            copy_library(&migration.source, &migration.target)?;
            config.root = Some(migration.target.clone());
            config.pending = None;
            if !config.aliases.contains(&migration.source) { config.aliases.push(migration.source.clone()); }
            save(path, &config)?;
            root = migration.target;
            Ok(())
        })();
        if let Err(error) = result { warning = Some(format!("文档库迁移未完成，仍使用原目录 / Library migration failed; using the original folder: {error:#}")); }
    }
    Ok((root, warning))
}

fn validate(source: &Path, target: &Path) -> Result<(PathBuf, PathBuf)> {
    let source = plain(&source.canonicalize().context("原文档库不可用 / Source library unavailable")?);
    let target = plain(&target.canonicalize().context("请选择已存在的空文件夹 / Select an existing empty folder")?);
    ensure!(source.is_dir() && target.is_dir(), "请选择文件夹 / Select a folder");
    ensure!(!target.starts_with(&source) && !source.starts_with(&target), "新旧文档库不能相同或互相包含 / Library folders must not overlap");
    ensure!(fs::read_dir(&target)?.next().is_none(), "目标文件夹必须为空，避免覆盖已有文件 / Destination must be empty to protect existing files");
    ensure!(target.parent().is_some(), "不能使用磁盘根目录 / Choose a folder, not a drive root");
    Ok((source, target))
}
fn schedule_at(path: &Path, source: &Path, target: &Path) -> Result<()> {
    let (source, target) = validate(source, target)?;
    // Test write access without leaving anything in the selected folder.
    let probe = target.join(format!(".mynote-write-{}", uuid::Uuid::new_v4()));
    fs::OpenOptions::new().create_new(true).write(true).open(&probe)?.sync_all()?;
    fs::remove_file(probe)?;
    let mut config = read(path)?;
    config.root = Some(source.clone());
    config.confirmed = true;
    config.pending = Some(Migration { source, target });
    save(path, &config)
}
pub fn schedule(target: &Path) -> Result<()> {
    ensure!(!overridden(), "QUIET_MD_HOME 指定了文档库，请先移除此环境变量 / Remove QUIET_MD_HOME before changing the location");
    schedule_at(&config_path(), &root_dir(), target)
}
pub fn pending() -> Option<PathBuf> { if overridden() { None } else { read(&config_path()).ok()?.pending.map(|p| p.target) } }
pub fn cancel() -> Result<()> {
    let path = config_path();
    let mut config = read(&path)?;
    config.pending = None;
    save(&path, &config)
}

fn copy_tree(source: &Path, target: &Path, depth: usize) -> Result<()> {
    ensure!(depth <= 128, "文档库目录层级过深 / Library directory nesting is too deep");
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if depth == 0 && entry.file_name() == ".lock" { continue; }
        let kind = entry.file_type()?;
        ensure!(!kind.is_symlink(), "请先移除文档库中的符号链接 / Remove symbolic links before migrating: {}", entry.path().display());
        let to = target.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&to)?;
            copy_tree(&entry.path(), &to, depth + 1)?;
        } else {
            ensure!(kind.is_file(), "文档库包含不支持迁移的文件 / Unsupported library file: {}", entry.path().display());
            let mut input = fs::File::open(entry.path())?;
            let mut output = fs::OpenOptions::new().create_new(true).write(true).open(&to)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            fs::set_permissions(&to, input.metadata()?.permissions())?;
        }
    }
    fs::set_permissions(target, fs::metadata(source)?.permissions())?;
    #[cfg(unix)]
    fs::File::open(target)?.sync_all()?;
    Ok(())
}
fn copy_library(source: &Path, target: &Path) -> Result<()> {
    let (source, target) = validate(source, target)?;
    ensure!(source.join("docs").is_dir() && source.join("format.json").is_file(), "原目录不是文档库 / Source is not a document library");
    // All Store writers use the same lock; no file changes during the snapshot.
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(source.join(".lock"))?;
    lock.try_lock().context("文档库正在写入，请退出其他实例后重试 / Library busy; close other instances and retry")?;
    let stage = target.parent().unwrap().join(format!(".mynote-migrate-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&stage)?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))?; }
    let result = (|| -> Result<()> {
        copy_tree(&source, &stage, 0)?;
        // remove_dir only succeeds for an empty target; never overwrite files.
        fs::remove_dir(&target)?;
        fs::rename(&stage, &target)?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_dir_all(&stage); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self { let path = std::env::temp_dir().join(format!("mynote-location-{}", uuid::Uuid::new_v4())); fs::create_dir_all(&path).unwrap(); Self(path) } }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn setup() -> (Temp, PathBuf, PathBuf, PathBuf) {
        let temp = Temp::new();
        let source = temp.0.join("原笔记 库");
        let target = temp.0.join("新位置");
        for dir in ["docs", "assets", "trash", ".nas-sync"] { fs::create_dir_all(source.join(dir)).unwrap(); }
        for (file, bytes) in [("docs/note.xml", "note"), ("assets/image.png", "image"), ("trash/index.json", "trash"), ("preferences.json", "prefs"), (".nas-sync/state.json", "state"), (".nas-sync/config.json", "secret"), ("format.json", "{\"format\":3}")] { fs::write(source.join(file), bytes).unwrap(); }
        fs::create_dir(&target).unwrap();
        let config = temp.0.join("settings/library.json");
        (temp, source, target, config)
    }
    #[test]
    fn migration_copies_latest_notes_and_all_local_state_and_retains_original() {
        let (_temp, source, target, config) = setup();
        schedule_at(&config, &source, &target).unwrap();
        fs::write(source.join("docs/note.xml"), "saved just before restart").unwrap();
        assert_eq!(read(&config).unwrap().root.unwrap(), plain(&source.canonicalize().unwrap()));
        let (root, warning) = initialize_at(&config, source.clone()).unwrap();
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(root, plain(&target.canonicalize().unwrap()));
        for file in ["docs/note.xml", "assets/image.png", "trash/index.json", "preferences.json", ".nas-sync/config.json", ".nas-sync/state.json", "format.json"] { assert_eq!(fs::read(source.join(file)).unwrap(), fs::read(target.join(file)).unwrap()); }
        assert!(read(&config).unwrap().pending.is_none());
        let old_asset = source.join("assets/image.png");
        assert_eq!(rebase(&old_asset, &read(&config).unwrap().aliases, &root), root.join("assets/image.png"));
        assert_eq!(rebase(&target.parent().unwrap().join("external.md"), &read(&config).unwrap().aliases, &root), target.parent().unwrap().join("external.md"));
        assert_eq!(initialize_at(&config, source).unwrap().0, root);
    }
    #[test]
    fn occupied_target_does_not_replace_any_files_or_change_active_library() {
        let (_temp, source, target, config) = setup();
        schedule_at(&config, &source, &target).unwrap();
        fs::write(target.join("important.txt"), "keep").unwrap();
        let (root, warning) = initialize_at(&config, source.clone()).unwrap();
        assert!(warning.is_some());
        assert_eq!(root, plain(&source.canonicalize().unwrap()));
        assert_eq!(fs::read_to_string(target.join("important.txt")).unwrap(), "keep");
        assert!(schedule_at(&config, &source, &target).is_err());
        assert!(!target.join("docs").exists());
    }
    #[test]
    fn rejects_overlapping_paths_missing_libraries_and_invalid_settings() {
        let (_temp, source, target, config) = setup();
        assert!(schedule_at(&config, &source, &source).is_err());
        assert!(schedule_at(&config, &source, &source.join("assets")).is_err());
        assert!(schedule_at(&config, &target, target.parent().unwrap()).is_err());
        save(&config, &Location { root: Some(source.join("missing")), ..Default::default() }).unwrap();
        assert!(initialize_at(&config, target.clone()).is_err());
        fs::write(&config, "broken json").unwrap();
        assert!(initialize_at(&config, target).is_err());
    }
    #[test]
    fn first_setup_is_explicit_persistent_and_does_not_overwrite_future_choices() {
        let (_temp, source, target, config) = setup();
        assert!(!read(&config).unwrap().confirmed);
        select_initial_at(&config, &target).unwrap();
        assert!(read(&config).unwrap().confirmed);
        assert!(read(&config).unwrap().create);
        assert_eq!(initialize_at(&config, source.clone()).unwrap().0, target);
        assert!(select_initial_at(&config, &source).is_err());
        assert!(fs::read_dir(&target).unwrap().next().is_none());
        assert!(validate_initial(source.parent().unwrap()).is_err());
        assert!(validate_initial(Path::new("relative/path")).is_err());
        let mut ready = read(&config).unwrap(); ready.create = false; save(&config, &ready).unwrap();
        // After first initialization a missing library must never be recreated.
        fs::remove_dir(&target).unwrap();
        assert!(initialize_at(&config, source).is_err());
    }
    #[test]
    fn portable_asset_paths_survive_relocation_without_old_folder() {
        let (_temp, source, target, _config) = setup();
        let relative = super::super::asset_source_in(&source, &source.join("assets/image.png"));
        assert_eq!(relative, "assets/image.png");
        copy_library(&source, &target).unwrap();
        fs::remove_dir_all(source).unwrap();
        assert_eq!(fs::read_to_string(target.join(relative)).unwrap(), "image");
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_never_copy_files_outside_the_library() {
        let (_temp, source, target, config) = setup();
        std::os::unix::fs::symlink(&config, source.join("assets/link")).unwrap();
        schedule_at(&config, &source, &target).unwrap();
        let (_, warning) = initialize_at(&config, source).unwrap();
        assert!(warning.is_some());
        assert!(fs::read_dir(target).unwrap().next().is_none());
    }
}
