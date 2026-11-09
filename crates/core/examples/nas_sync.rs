//! Headless sync for diagnostics; uses the same protocol and storage as the UI.
use fdl_core::store::{Store, nas_sync::{self, Config}};
fn main() -> anyhow::Result<()> {
    let root = Store::root_dir();
    let config = Config::load(&root)?;
    let pending = nas_sync::prepare(root, config)?;
    let report = pending.apply(&mut Store::open()?)?;
    println!("changed={} conflicts={} skipped={}", report.changed, report.conflicts, report.skipped);
    Ok(())
}
