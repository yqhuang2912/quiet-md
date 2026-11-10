//! Resumable update transfers. A failed connection keeps its bytes; only a
//! package matching the manifest's size, name and signature can be installed.

use super::{Manifest, command, signed};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub(super) fn fetch(m: &Manifest, name: &str, dir: &Path, key: &minisign_verify::PublicKey) -> Result<PathBuf, String> {
    fetch_with(m, name, dir, key, 5, Duration::from_secs(1))
}

fn fetch_with(m: &Manifest, name: &str, dir: &Path, key: &minisign_verify::PublicKey, attempts: usize, delay: Duration) -> Result<PathBuf, String> {
    let (file, part, meta) = (dir.join(name), dir.join(format!("{name}.part")), dir.join(format!("{name}.json")));
    let previous = fs::read(&meta).ok().and_then(|b| serde_json::from_slice::<Manifest>(&b).ok());
    if previous.as_ref().is_some_and(|old| old != m) {
        for p in [&part, &file] { remove(p)?; }
    }
    // A legacy partial file has no sidecar; its signed name and final signature
    // still bind it to this release. Write the identity before adding any bytes.
    if previous.as_ref() != Some(m) { fs::write(&meta, serde_json::to_vec(m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?; }
    if file.is_file() {
        if verify(&file, m, name, key).is_ok() { return Ok(file) }
        remove(&file)?;
    }
    if size(&part) > m.size { remove(&part)?; }
    let mut last = String::new();
    let mut restarted = false;
    for attempt in 0..attempts {
        let offset = size(&part);
        if offset == m.size { break }
        if attempt > 0 { std::thread::sleep(delay.saturating_mul(1 << (attempt - 1).min(3))); }
        // Keep paths out of curl's arguments: some Windows curl builds use the
        // ANSI code page. Resume uses an explicit byte offset and an append handle.
        let out = OpenOptions::new().create(true).append(true).open(&part).map_err(|e| e.to_string())?;
        let protocols = if m.url.starts_with("http://") { "=https,http" } else { "=https" };
        let mut c = curl(&m.url);
        c.args(["-fsSL", "--proto", protocols, "--proto-redir", protocols, "--connect-timeout", "60", "--max-time", "1800", "--speed-limit", "1", "--speed-time", "60", "--max-filesize", &m.size.to_string(), "--write-out", "%{stderr}\nquiet-md-http:%{http_code}"]);
        if offset > 0 { c.args(["--continue-at", &offset.to_string()]); }
        let response = c.arg(&m.url).stdin(Stdio::null()).stdout(Stdio::from(out)).stderr(Stdio::piped()).output().map_err(|e| format!("curl: {e}"))?;
        let stderr = String::from_utf8_lossy(&response.stderr);
        let (message, status) = stderr.rsplit_once("\nquiet-md-http:").unwrap_or((&stderr, ""));
        let code = response.status.code();
        if size(&part) > m.size { remove(&part)?; return Err(verification_error("size")) }
        if response.status.success() {
            if size(&part) == m.size { break }
            last = verification_error("incomplete download");
        } else {
            last = super::curl_error(message.as_bytes());
            // Some servers refuse Range. Retry once from zero rather than
            // appending a whole response to a partial package.
            if offset > 0 && !restarted && (code == Some(33) || status.trim() == "416") {
                remove(&part)?;
                restarted = true;
            } else if !matches!(code, Some(5 | 6 | 7 | 16 | 18 | 28 | 35 | 52 | 55 | 56 | 81 | 92 | 95))
                && !(code == Some(22) && matches!(status.trim(), "408" | "429" | "500" | "502" | "503" | "504")) { return Err(last) }
        }
    }
    if size(&part) != m.size {
        return Err(format!("{last}\n{}", crate::settings::tr("已保留下载进度，可点击继续下载", "Download progress was saved. Click Resume download to continue.")));
    }
    if let Err(e) = verify(&part, m, name, key) {
        remove(&part)?;
        return Err(e);
    }
    fs::rename(&part, &file).map_err(|e| e.to_string())?;
    Ok(file)
}

fn size(path: &Path) -> u64 { fs::metadata(path).map_or(0, |m| m.len()) }
fn remove(path: &Path) -> Result<(), String> { match fs::remove_file(path) { Ok(()) => Ok(()), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(e) => Err(e.to_string()) } }
fn verification_error(what: &str) -> String { format!("{}: {what}", crate::settings::tr("更新包校验失败", "The update package failed verification")) }
fn verify(path: &Path, m: &Manifest, name: &str, key: &minisign_verify::PublicKey) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != m.size { return Err(verification_error("size")) }
    if !signed(&bytes, &m.signature, name, key) { return Err(verification_error("signature")) }
    Ok(())
}

struct Proxy { url: String, bypass: String }

/// GUI launches do not inherit a terminal's proxy environment. Honour an
/// explicitly configured environment first, then the OS's manual proxy.
pub(super) fn curl(url: &str) -> Command {
    let mut c = command("curl");
    let scheme = url.split_once("://").map_or("https", |(s, _)| s);
    let authority = url.split_once("://").map_or("", |(_, r)| r).split('/').next().unwrap_or("");
    let local = authority.starts_with("127.") || authority.starts_with("localhost:") || authority.starts_with("[::1]");
    let vars = [format!("{scheme}_proxy"), format!("{}_PROXY", scheme.to_uppercase()), "all_proxy".into(), "ALL_PROXY".into()];
    if local { c.args(["--noproxy", "*"]); }
    else if !vars.iter().any(|v| std::env::var_os(v).is_some()) && let Some(proxy) = system_proxy(scheme) {
        c.args(["--proxy", &proxy.url]);
        if !proxy.bypass.is_empty() && std::env::var_os("no_proxy").is_none() && std::env::var_os("NO_PROXY").is_none() { c.args(["--noproxy", &proxy.bypass]); }
    }
    c
}

#[cfg(target_os = "windows")]
fn system_proxy(scheme: &str) -> Option<Proxy> {
    let key = windows_registry::CURRENT_USER.open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings").ok()?;
    if key.get_u32("ProxyEnable").ok()? == 0 { return None }
    let server = key.get_string("ProxyServer").ok()?;
    let url = windows_proxy(&server, scheme)?;
    let bypass = key.get_string("ProxyOverride").unwrap_or_default().split(';').filter(|s| !s.starts_with('<')).map(|s| s.trim().replace("*.", ".")).collect::<Vec<_>>().join(",");
    Some(Proxy { url, bypass })
}

#[cfg(any(target_os = "windows", test))]
fn windows_proxy(server: &str, scheme: &str) -> Option<String> {
    let server = if server.contains('=') { server.split(';').find_map(|p| p.trim().split_once('=').filter(|(s, _)| s.eq_ignore_ascii_case(scheme)).map(|(_, p)| p))? } else { server };
    let server = server.trim();
    if server.is_empty() { return None }
    Some(if server.contains("://") { server.into() } else { format!("http://{server}") })
}

#[cfg(target_os = "macos")]
fn system_proxy(scheme: &str) -> Option<Proxy> {
    let out = command("/usr/sbin/scutil").arg("--proxy").stdin(Stdio::null()).output().ok()?;
    if !out.status.success() { return None }
    mac_proxy(&String::from_utf8_lossy(&out.stdout), scheme)
}

#[cfg(any(target_os = "macos", test))]
fn mac_proxy(text: &str, scheme: &str) -> Option<Proxy> {
    let value = |key: &str| text.lines().filter(|l| l.starts_with("  ") && !l.starts_with("    "))
        .find_map(|l| l.trim().split_once(" : ").filter(|(k, _)| *k == key).map(|(_, v)| v.trim()));
    let prefix = if scheme == "https" { "HTTPS" } else { "HTTP" };
    let (prefix, protocol) = if value(&format!("{prefix}Enable")) == Some("1") { (prefix, "http") }
        else if value("SOCKSEnable") == Some("1") { ("SOCKS", "socks5h") } else { return None };
    let host = value(&format!("{prefix}Proxy"))?;
    let port = value(&format!("{prefix}Port"))?.parse::<u16>().ok().filter(|p| *p > 0)?;
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.into() };
    let mut in_exceptions = false;
    let mut bypass = vec![];
    for l in text.lines() {
        if l.trim_start().starts_with("ExceptionsList : ") { in_exceptions = true; continue }
        if in_exceptions && l.trim() == "}" { break }
        if in_exceptions && let Some((_, host)) = l.trim().split_once(" : ") { bypass.push(host.replace("*.", ".")); }
    }
    Some(Proxy { url: format!("{protocol}://{host}:{port}"), bypass: bypass.join(",") })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn system_proxy(_: &str) -> Option<Proxy> { None }

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{TEST_KEY, TEST_SIG};
    use std::io::{Read, Write};

    const NAME: &str = "quiet-md-0.2.0-macos-arm64.zip";
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("mynote-download-{}", uuid::Uuid::new_v4())).join("中文 下载");
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn part(&self) -> PathBuf { self.0.join(format!("{NAME}.part")) }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(self.0.parent().unwrap()); } }

    fn server(responses: Vec<String>) -> (Manifest, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let m = Manifest { version: "0.2.0".into(), url: format!("http://{}/{NAME}", listener.local_addr().unwrap()), size: 8, signature: TEST_SIG.into() };
        let task = std::thread::spawn(move || {
            let mut requests = vec![];
            for response in responses {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline, "HTTP request missing");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = vec![];
                while !request.ends_with(b"\r\n\r\n") {
                    let mut buf = [0; 1024];
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buf[..n]);
                }
                requests.push(String::from_utf8(request).unwrap().to_lowercase());
                socket.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (m, task)
    }
    fn full() -> String { "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nquiet.md".into() }
    fn interrupted() -> String { "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nqui".into() }
    fn resumed() -> String { "HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 3-7/8\r\nConnection: close\r\n\r\net.md".into() }
    fn fetch_at(m: &Manifest, temp: &Temp, attempts: usize) -> Result<PathBuf, String> {
        fetch_with(m, NAME, &temp.0, &minisign_verify::PublicKey::from_base64(TEST_KEY).unwrap(), attempts, Duration::ZERO)
    }

    #[test]
    fn interrupted_download_retries_from_its_saved_offset() {
        let temp = Temp::new();
        let (m, task) = server(vec![interrupted(), resumed()]);
        let file = fetch_at(&m, &temp, 3).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"quiet.md");
        let requests = task.join().unwrap();
        assert!(!requests[0].contains("range:"));
        assert!(requests[1].contains("range: bytes=3-"));
        assert!(!temp.part().exists());
        // A verified completed package needs no further network connection.
        assert_eq!(fetch_at(&m, &temp, 1).unwrap(), file);
    }

    #[test]
    fn failed_attempt_survives_an_explicit_retry() {
        let temp = Temp::new();
        let (m, task) = server(vec![interrupted(), resumed()]);
        assert!(fetch_at(&m, &temp, 1).is_err());
        assert_eq!(fs::read(temp.part()).unwrap(), b"qui");
        assert_eq!(fs::read(fetch_at(&m, &temp, 1).unwrap()).unwrap(), b"quiet.md");
        assert!(task.join().unwrap()[1].contains("range: bytes=3-"));
    }

    #[test]
    fn legacy_partial_resumes_and_range_refusal_restarts_safely() {
        for refusal in [false, true] {
            let temp = Temp::new();
            fs::write(temp.part(), b"qui").unwrap();
            let (m, task) = server(if refusal { vec![full(), full()] } else { vec![resumed()] });
            assert_eq!(fs::read(fetch_at(&m, &temp, 3).unwrap()).unwrap(), b"quiet.md");
            let requests = task.join().unwrap();
            assert!(requests[0].contains("range: bytes=3-"));
            if refusal { assert!(!requests[1].contains("range:")); }
        }
    }

    #[test]
    fn changed_manifest_cannot_reuse_another_downloads_bytes() {
        let temp = Temp::new();
        let (m, task) = server(vec![full()]);
        let mut old = m.clone();
        old.url.push_str("?old");
        fs::write(temp.0.join(format!("{NAME}.json")), serde_json::to_vec(&old).unwrap()).unwrap();
        fs::write(temp.part(), b"qui").unwrap();
        assert_eq!(fs::read(fetch_at(&m, &temp, 1).unwrap()).unwrap(), b"quiet.md");
        assert!(!task.join().unwrap()[0].contains("range:"));
    }

    #[test]
    fn corrupt_complete_partial_is_rejected_and_removed() {
        let temp = Temp::new();
        fs::write(temp.part(), b"quiet.mD").unwrap();
        let m = Manifest { version: "0.2.0".into(), url: format!("http://127.0.0.1:1/{NAME}"), size: 8, signature: TEST_SIG.into() };
        assert!(fetch_at(&m, &temp, 1).unwrap_err().contains("signature"));
        assert!(!temp.part().exists());
        assert!(!temp.0.join(NAME).exists());
    }

    #[test]
    fn transient_http_failure_keeps_the_partial_and_retries() {
        let temp = Temp::new();
        fs::write(temp.part(), b"qui").unwrap();
        let (m, task) = server(vec!["HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(), resumed()]);
        assert_eq!(fs::read(fetch_at(&m, &temp, 3).unwrap()).unwrap(), b"quiet.md");
        assert!(task.join().unwrap().iter().all(|r| r.contains("range: bytes=3-")));
    }

    #[test]
    #[ignore = "Downloads a real release into a temporary cache; requires MYNOTE_TEST_MANIFEST"]
    fn public_release_download_uses_the_normal_proxy_and_signature_path() {
        let temp = Temp::new();
        let path = std::env::var_os("MYNOTE_TEST_MANIFEST").expect("MYNOTE_TEST_MANIFEST");
        let m: Manifest = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let platform = super::super::platform().unwrap();
        super::super::validate(&m, platform, false).unwrap();
        let name = super::super::package_name(&m.version, platform);
        let key = super::super::public_key().unwrap();
        if let Some(seed) = std::env::var_os("MYNOTE_TEST_SEED_PACKAGE") {
            let seed = PathBuf::from(seed);
            verify(&seed, &m, &name, &key).unwrap();
            let bytes = fs::read(seed).unwrap();
            let at = bytes.len() * 9 / 10;
            fs::write(temp.0.join(format!("{name}.part")), &bytes[..at]).unwrap();
            eprintln!("Resuming a real release from {at}/{} bytes", m.size);
        }
        let started = std::time::Instant::now();
        let file = fetch(&m, &name, &temp.0, &key).unwrap();
        assert_eq!(fs::metadata(file).unwrap().len(), m.size);
        eprintln!("Verified {} bytes through the actual updater in {:.1}s", m.size, started.elapsed().as_secs_f64());
    }

    #[test]
    fn manual_system_proxies_use_the_matching_protocol() {
        assert_eq!(windows_proxy("127.0.0.1:7897", "https").as_deref(), Some("http://127.0.0.1:7897"));
        assert_eq!(windows_proxy("http=proxy:80;https=secure:81", "https").as_deref(), Some("http://secure:81"));
        assert!(windows_proxy("http=proxy:80", "https").is_none());
        assert!(windows_proxy(" ", "https").is_none());
        let text = "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n    1 : 127.0.0.1\n  }\n  HTTPSEnable : 1\n  HTTPSPort : 7897\n  HTTPSProxy : 127.0.0.1\n  HTTPEnable : 0\n}";
        let proxy = mac_proxy(text, "https").unwrap();
        assert_eq!(proxy.url, "http://127.0.0.1:7897");
        assert_eq!(proxy.bypass, ".local,127.0.0.1");
        assert!(mac_proxy(text, "http").is_none());
        assert!(mac_proxy("<dictionary> {\n  Scoped : <dictionary> {\n    HTTPSEnable : 1\n    HTTPSProxy : other\n    HTTPSPort : 80\n  }\n}", "https").is_none());
        assert_eq!(mac_proxy("<dictionary> {\n  SOCKSEnable : 1\n  SOCKSProxy : ::1\n  SOCKSPort : 1080\n}", "https").unwrap().url, "socks5h://[::1]:1080");
    }
}
