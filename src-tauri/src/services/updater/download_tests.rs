use super::*;
use std::fs;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

const IDLE: Duration = Duration::from_millis(500);

struct Download {
    server: TcpListener,
    directory: TempDir,
    http: reqwest::Client,
}

impl Download {
    async fn new() -> Self {
        Self {
            server: TcpListener::bind("127.0.0.1:0").await.unwrap(),
            directory: tempfile::tempdir().unwrap(),
            http: reqwest::Client::builder().no_proxy().build().unwrap(),
        }
    }

    async fn fetch(&self, expected: Option<&str>) -> Result<PathBuf, String> {
        let info = UpdateInfo {
            version: "9.9.9".into(),
            url: format!("http://{}/setup.exe", self.server.local_addr().unwrap()),
            notes: None,
            sha256: expected.map(str::to_owned),
        };
        download_installer(&self.http, &info, self.directory.path(), IDLE).await
    }

    async fn respond(&self, length: usize, prefix: &[u8]) -> TcpStream {
        let (socket, _) = timeout(Duration::from_secs(5), self.server.accept())
            .await
            .expect("installer request should reach the server")
            .unwrap();
        let mut reader = BufReader::new(socket);
        let mut headers = String::new();
        while !headers.ends_with("\r\n\r\n") {
            assert_ne!(reader.read_line(&mut headers).await.unwrap(), 0);
        }
        assert!(headers.starts_with("GET /setup.exe "));
        let mut socket = reader.into_inner();
        let headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n\r\n");
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(prefix).await.unwrap();
        socket
    }

    fn files(&self) -> Vec<PathBuf> {
        fs::read_dir(self.directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    async fn wait_for_partial(&self) {
        timeout(Duration::from_secs(2), async {
            while !self.files().iter().any(|path| {
                path.extension().is_some_and(|ext| ext == "part")
                    && fs::File::open(path).unwrap().metadata().unwrap().len() > 0
            }) {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the first bytes should reach the temporary file before EOF");
    }
}

#[tokio::test]
async fn next_download_removes_only_old_updater_partials() {
    let test = Download::new().await;
    let dir = test.directory.path();
    let old = [
        dir.join("VibeUsage-abC123.part"),
        dir.join("VibeUsage-987XYZ.part"),
    ];
    let keep = [
        dir.join("OtherApp-abC123.part"),
        dir.join("VibeUsage-9.9.9-Setup.exe.part"),
        dir.join("VibeUsage-9.9.8-Setup.exe"),
    ];
    for path in old.iter().chain(&keep) {
        fs::write(path, b"previous").unwrap();
    }
    let subdir = dir.join("VibeUsage-Dir123.part");
    fs::create_dir(&subdir).unwrap();
    let nested = subdir.join("VibeUsage-abC123.part");
    fs::write(&nested, b"nested").unwrap();

    let digest = hex::encode(Sha256::digest(b"new"));
    let serve = async {
        let _socket = test.respond(3, b"new").await;
        assert!(
            old.iter().all(|path| !path.exists()),
            "stale downloads survived"
        );
    };
    let (result, ()) = tokio::join!(test.fetch(Some(&digest)), serve);
    assert_eq!(fs::read(result.unwrap()).unwrap(), b"new");
    for path in keep {
        assert_eq!(fs::read(path).unwrap(), b"previous");
    }
    assert_eq!(fs::read(nested).unwrap(), b"nested");
}

#[tokio::test]
async fn rename_retries_access_denied_and_sharing_violation_on_the_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let partial = tempfile::NamedTempFile::new_in(&dir)
        .unwrap()
        .into_temp_path();
    fs::write(&partial, b"verified").unwrap();
    let installer = dir.path().join("setup.exe");
    fs::write(&installer, b"previous").unwrap();

    let mut errors = [5, 32].into_iter();
    let started = std::time::Instant::now();
    persist_installer(partial, |path| {
        if let Some(code) = errors.next() {
            return Err(tempfile::PathPersistError {
                error: std::io::Error::from_raw_os_error(code),
                path,
            });
        }
        path.persist(&installer)
    })
    .await
    .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert_eq!(fs::read(installer).unwrap(), b"verified");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn rename_returns_the_last_error_and_discards_the_partial() {
    for (code, expected_attempts) in [(32, 6), (2, 1)] {
        let dir = tempfile::tempdir().unwrap();
        let partial = tempfile::NamedTempFile::new_in(&dir)
            .unwrap()
            .into_temp_path();
        let mut attempts = 0;
        let err = persist_installer(partial, |path| {
            attempts += 1;
            assert!(attempts <= expected_attempts);
            Err(tempfile::PathPersistError {
                error: std::io::Error::from_raw_os_error(code),
                path,
            })
        })
        .await
        .unwrap_err();
        assert_eq!(attempts, expected_attempts);
        assert_eq!(err.raw_os_error(), Some(code));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn slow_continuous_download_writes_before_eof_and_outlasts_the_old_deadline() {
    let test = Download::new().await;
    let body = [b'x'; 61];
    let digest = hex::encode_upper(Sha256::digest(body));
    let installer = test.directory.path().join("VibeUsage-9.9.9-Setup.exe");
    let serve = async {
        let mut socket = test.respond(body.len(), &body[..1]).await;
        // Scale 60 s idle / 600 s total to 0.5 s / 5 s. Deliver over 6 s.
        for (i, byte) in body[1..].iter().enumerate() {
            sleep(Duration::from_millis(100)).await;
            if i == 30 {
                assert!(!installer.exists());
                test.wait_for_partial().await;
            }
            socket.write_all(&[*byte]).await.unwrap();
        }
    };
    let (result, ()) = tokio::join!(test.fetch(Some(&digest)), serve);
    assert_eq!(result.unwrap(), installer);
    assert_eq!(fs::read(&installer).unwrap(), body);
    assert_eq!(test.files(), [installer]);
}

#[tokio::test]
async fn stalled_download_times_out_and_removes_partial_file() {
    let test = Download::new().await;
    let digest = hex::encode(Sha256::digest(b"ab"));
    let serve = async {
        let mut socket = test.respond(2, b"a").await;
        test.wait_for_partial().await;
        sleep(IDLE * 3).await;
        let _ = socket.write_all(b"b").await;
    };
    let (result, ()) = tokio::join!(test.fetch(Some(&digest)), serve);
    assert!(result.unwrap_err().contains("无进展"));
    assert!(test.files().is_empty());
}

#[tokio::test]
async fn silent_server_times_out_before_headers() {
    let test = Download::new().await;
    let digest = hex::encode(Sha256::digest(b"a"));
    // Leave the listener open without sending a response.
    let result = timeout(IDLE * 4, test.fetch(Some(&digest)))
        .await
        .expect("waiting for headers must have a deadline");
    assert!(result.unwrap_err().contains("无进展"));
    assert!(test.files().is_empty());
}

#[tokio::test]
async fn truncated_response_fails_and_removes_partial_file() {
    let test = Download::new().await;
    // Even a hash matching the bytes received cannot excuse a broken response.
    let digest = hex::encode(Sha256::digest(b"a"));
    let serve = async {
        let socket = test.respond(2, b"a").await;
        test.wait_for_partial().await;
        drop(socket);
    };
    let (result, ()) = tokio::join!(test.fetch(Some(&digest)), serve);
    assert!(result.unwrap_err().starts_with("下载失败"));
    assert!(test.files().is_empty());
}

#[tokio::test]
async fn wrong_hash_removes_partial_file_and_preserves_existing_installer() {
    let test = Download::new().await;
    let installer = test.directory.path().join("VibeUsage-9.9.9-Setup.exe");
    fs::write(&installer, b"previous").unwrap();
    let digest = hex::encode(Sha256::digest(b"expected"));
    let serve = async {
        let mut socket = test.respond(5, b"w").await;
        test.wait_for_partial().await;
        socket.write_all(b"rong").await.unwrap();
    };
    let (result, ()) = tokio::join!(test.fetch(Some(&digest)), serve);
    assert_eq!(result.unwrap_err(), "安装包校验失败 (SHA-256 不匹配)");
    assert_eq!(fs::read(&installer).unwrap(), b"previous");
    assert_eq!(test.files(), [installer]);
}

#[tokio::test]
async fn missing_hash_is_rejected_before_downloading() {
    let test = Download::new().await;
    tokio::select! {
        result = test.fetch(None) => assert!(result.unwrap_err().contains("SHA-256")),
        _ = test.server.accept() => panic!("requested an installer with no digest"),
    }
    assert!(test.files().is_empty());
}
