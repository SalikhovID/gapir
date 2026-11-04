//! Downloads the speech model with resume support and verifies it before first use.
//! `install.sh` uses the same paths, so an interrupted terminal download resumes here too.

use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    pub url: &'static str,
    pub file_name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

#[derive(Debug)]
pub enum DownloadError {
    Network(String),
    Io(std::io::Error),
    HashMismatch,
    Cancelled,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(reason) => write!(f, "network error: {reason}"),
            Self::Io(error) => write!(f, "file error: {error}"),
            Self::HashMismatch => write!(f, "downloaded file is damaged"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<std::io::Error> for DownloadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn model_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(spec.file_name)
}

fn part_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(format!("{}.part", spec.file_name))
}

/// Cheap check for startup. Full integrity is checked after download and on load failure.
pub fn is_present(dir: &Path, spec: &ModelSpec) -> bool {
    fs::metadata(model_path(dir, spec)).is_ok_and(|m| m.len() == spec.size)
}

pub fn verify(dir: &Path, spec: &ModelSpec) -> std::io::Result<bool> {
    Ok(sha256_hex(&model_path(dir, spec))? == spec.sha256)
}

fn sha256_hex(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Downloads the model into `dir`, resuming a `.part` file if there is one.
/// On network failure the `.part` file stays for the next attempt.
pub fn download(
    dir: &Path,
    spec: &ModelSpec,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, DownloadError> {
    let target = model_path(dir, spec);
    if is_present(dir, spec) {
        return Ok(target);
    }
    fs::create_dir_all(dir)?;
    let part = part_path(dir, spec);
    let have = fs::metadata(&part).map_or(0, |m| m.len());
    if have < spec.size {
        fetch(spec, &part, have, &mut progress, cancel)?;
    }
    if sha256_hex(&part)? != spec.sha256 {
        fs::remove_file(&part)?;
        return Err(DownloadError::HashMismatch);
    }
    fs::rename(&part, &target)?;
    Ok(target)
}

fn fetch(
    spec: &ModelSpec,
    part: &Path,
    have: u64,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let network = |e: &dyn std::fmt::Display| DownloadError::Network(e.to_string());
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .build(),
    );
    let mut request = agent.get(spec.url);
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let mut response = request.call().map_err(|e| network(&e))?;
    let (mut file, mut done) = match response.status().as_u16() {
        206 => (OpenOptions::new().append(true).open(part)?, have),
        200 => (File::create(part)?, 0),
        status => return Err(DownloadError::Network(format!("HTTP {status}"))),
    };

    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let read = reader.read(&mut buffer).map_err(|e| network(&e))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        done += read as u64;
        progress(done, spec.size);
    }
    if done < spec.size {
        return Err(DownloadError::Network("connection closed early".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Ranges,
        IgnoreRanges,
        /// Promise the whole body, send half, close the connection.
        Truncate,
    }

    struct Stub {
        url: &'static str,
        requests: Arc<Mutex<Vec<String>>>,
    }

    /// Minimal HTTP server, just enough for the download client.
    fn serve(body: Vec<u8>, mode: Mode) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.gguf", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).to_lowercase();
                seen.lock().unwrap().push(head.clone());
                let start = head
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
                let response = match (mode, start) {
                    (Mode::Ranges, Some(start)) if start >= body.len() => {
                        b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                    }
                    (Mode::Ranges, Some(start)) => {
                        let part = &body[start..];
                        let mut r = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                            part.len(), start, body.len() - 1, body.len()
                        ).into_bytes();
                        r.extend_from_slice(part);
                        r
                    }
                    (Mode::Truncate, _) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ).into_bytes();
                        r.extend_from_slice(&body[..body.len() / 2]);
                        r
                    }
                    _ => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ).into_bytes();
                        r.extend_from_slice(&body);
                        r
                    }
                };
                let _ = stream.write_all(&response);
            }
        });
        Stub {
            url: Box::leak(url.into_boxed_str()),
            requests,
        }
    }

    fn body() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn sha(bytes: &[u8]) -> &'static str {
        let hex: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Box::leak(hex.into_boxed_str())
    }

    fn spec(url: &'static str, bytes: &[u8]) -> ModelSpec {
        ModelSpec {
            url,
            file_name: "model.gguf",
            size: bytes.len() as u64,
            sha256: sha(bytes),
        }
    }

    fn get(dir: &Path, spec: &ModelSpec) -> Result<PathBuf, DownloadError> {
        download(dir, spec, |_, _| {}, &AtomicBool::new(false))
    }

    fn part_path(dir: &Path) -> PathBuf {
        dir.join("model.gguf.part")
    }

    #[test]
    fn downloads_a_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let spec = spec(serve(bytes.clone(), Mode::Ranges).url, &bytes);
        let path = get(dir.path(), &spec).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(!part_path(dir.path()).exists());
        assert!(is_present(dir.path(), &spec));
    }

    #[test]
    fn resumes_from_a_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        std::fs::write(part_path(dir.path()), &bytes[..50_000]).unwrap();
        let stub = serve(bytes.clone(), Mode::Ranges);
        let spec = spec(stub.url, &bytes);
        let path = get(dir.path(), &spec).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(stub.requests.lock().unwrap()[0].contains("range: bytes=50000-"));
    }

    #[test]
    fn restarts_when_the_server_ignores_the_range() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        std::fs::write(part_path(dir.path()), &bytes[..50_000]).unwrap();
        let spec = spec(serve(bytes.clone(), Mode::IgnoreRanges).url, &bytes);
        let path = get(dir.path(), &spec).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn a_complete_partial_file_needs_no_more_data() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        std::fs::write(part_path(dir.path()), &bytes).unwrap();
        let spec = spec(serve(bytes.clone(), Mode::Ranges).url, &bytes);
        let path = get(dir.path(), &spec).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn wrong_hash_removes_the_partial_file_and_fails() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let mut spec = spec(serve(bytes.clone(), Mode::Ranges).url, &bytes);
        spec.sha256 = "0000";
        assert!(matches!(
            get(dir.path(), &spec),
            Err(DownloadError::HashMismatch)
        ));
        assert!(!part_path(dir.path()).exists());
        assert!(!model_path(dir.path(), &spec).exists());
    }

    #[test]
    fn interrupted_transfer_keeps_the_partial_file_for_later() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let spec = spec(serve(bytes.clone(), Mode::Truncate).url, &bytes);
        assert!(matches!(
            get(dir.path(), &spec),
            Err(DownloadError::Network(_))
        ));
        let kept = std::fs::metadata(part_path(dir.path())).unwrap().len();
        assert!(kept > 0 && kept < bytes.len() as u64, "kept {kept}");
    }

    #[test]
    fn progress_rises_to_the_total() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let spec = spec(serve(bytes.clone(), Mode::Ranges).url, &bytes);
        let mut seen = Vec::new();
        download(
            dir.path(),
            &spec,
            |done, total| seen.push((done, total)),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(seen.last(), Some(&(bytes.len() as u64, bytes.len() as u64)));
    }

    #[test]
    fn cancel_stops_the_download_and_keeps_the_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let spec = spec(serve(bytes.clone(), Mode::Ranges).url, &bytes);
        let cancel = AtomicBool::new(true);
        let result = download(dir.path(), &spec, |_, _| {}, &cancel);
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        cancel.store(false, Ordering::SeqCst);
        assert!(!model_path(dir.path(), &spec).exists());
    }

    #[test]
    fn present_file_of_the_right_size_is_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let stub = serve(bytes.clone(), Mode::Ranges);
        let spec = spec(stub.url, &bytes);
        std::fs::write(model_path(dir.path(), &spec), &bytes).unwrap();
        get(dir.path(), &spec).unwrap();
        assert!(stub.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn wrong_size_file_is_not_present() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let spec = spec("http://unused.invalid/", &bytes);
        std::fs::write(model_path(dir.path(), &spec), b"short").unwrap();
        assert!(!is_present(dir.path(), &spec));
    }

    #[test]
    fn verify_detects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = body();
        let spec = spec("http://unused.invalid/", &bytes);
        let path = model_path(dir.path(), &spec);
        std::fs::write(&path, &bytes).unwrap();
        assert!(verify(dir.path(), &spec).unwrap());
        let mut damaged = bytes;
        damaged[100] ^= 1;
        std::fs::write(&path, &damaged).unwrap();
        assert!(!verify(dir.path(), &spec).unwrap());
    }
}
