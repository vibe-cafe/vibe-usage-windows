use base64::{engine::general_purpose::STANDARD, Engine};
use tokio::io::{copy, copy_bidirectional, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use url::Url;
use vibe_core::{claude_relay::ClaudeRelay, proxy_route::Route};

async fn listener() -> TcpListener {
    TcpListener::bind(("127.0.0.1", 0)).await.unwrap()
}

async fn header(reader: &mut BufReader<TcpStream>) -> String {
    let mut data = Vec::new();
    while !data.ends_with(b"\r\n\r\n") {
        data.push(reader.read_u8().await.unwrap());
        assert!(data.len() <= 8192);
    }
    String::from_utf8(data).unwrap()
}

async fn request(
    url: &Url,
    method: &str,
    target: &str,
    auth: &str,
    payload: &[u8],
) -> (u16, BufReader<TcpStream>) {
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    let mut client = BufReader::new(
        TcpStream::connect(("127.0.0.1", url.port().unwrap()))
            .await
            .unwrap(),
    );
    let mut message = format!("{method} {target} HTTP/1.1\r\n{auth}\r\n").into_bytes();
    message.extend_from_slice(payload);
    client.write_all(&message).await.unwrap();
    let status = header(&mut client)
        .await
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, client)
}

fn credential(url: &Url) -> String {
    let basic = STANDARD.encode(format!("{}:{}", url.username(), url.password().unwrap()));
    format!("Proxy-Authorization: Basic {basic}\r\n")
}

#[tokio::test]
async fn refuses_other_processes_destinations_methods_and_unresolved_routes() {
    let relay = ClaudeRelay::start(&[("api.anthropic.com".into(), Route::Unresolved)])
        .await
        .unwrap();
    let other = ClaudeRelay::start(&[]).await.unwrap();
    let url = Url::parse(relay.url()).unwrap();
    let auth = credential(&url);
    let wrong = credential(&Url::parse(other.url()).unwrap());
    assert_ne!(auth, wrong);
    for denied in ["", &wrong] {
        assert_eq!(
            request(&url, "CONNECT", "api.anthropic.com:443", denied, b"")
                .await
                .0,
            407
        );
    }
    for denied in [
        "evilclaude.ai:443",
        "claude.com.example:443",
        "claude.ai:80",
        "127.0.0.1:443",
        "example.com/path.anthropic.com:443",
        "example.com@api.claude.com:443",
    ] {
        assert_eq!(request(&url, "CONNECT", denied, &auth, b"").await.0, 403);
    }
    assert_eq!(
        request(&url, "GET", "claude.ai:443", &auth, b"").await.0,
        405
    );
    assert_eq!(
        request(&url, "CONNECT", "api.anthropic.com:443", &auth, b"")
            .await
            .0,
        502
    );
    other.stop().await;
    relay.stop().await;
}

#[tokio::test]
async fn cancelling_a_probe_closes_an_incomplete_connect_and_the_listener() {
    let relay = ClaudeRelay::start(&[]).await.unwrap();
    let url = Url::parse(relay.url()).unwrap();
    let address = ("127.0.0.1", url.port().unwrap());
    let mut client = TcpStream::connect(address).await.unwrap();
    client
        .write_all(b"CONNECT claude.ai:443 HTTP/1.1\r\n")
        .await
        .unwrap();
    drop(relay);
    let closed = tokio::time::timeout(std::time::Duration::from_secs(2), client.read_u8())
        .await
        .unwrap();
    assert!(closed.is_err());
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn pinned_socks_and_http_routes_tunnel_to_echo_and_close_with_the_probe() {
    tokio::time::timeout(std::time::Duration::from_secs(10), check_pinned_routes())
        .await
        .unwrap();
}

async fn check_pinned_routes() {
    let echo = listener().await;
    let echo_addr = echo.local_addr().unwrap();
    let echo_task = tokio::spawn(async move {
        let mut pipes = tokio::task::JoinSet::new();
        for _ in 0..2 {
            let (mut stream, _) = echo.accept().await.unwrap();
            pipes.spawn(async move {
                let (mut read, mut write) = stream.split();
                copy(&mut read, &mut write).await.unwrap();
            });
        }
        while let Some(result) = pipes.join_next().await {
            result.unwrap();
        }
    });
    let socks = listener().await;
    let http = listener().await;
    let mut routes = vec![
        (
            "api.anthropic.com".into(),
            Route::Socks(socks.local_addr().unwrap().to_string()),
        ),
        (
            "claude.ai".into(),
            Route::Http(format!(
                "http://alice:p%40ss@{}",
                http.local_addr().unwrap()
            )),
        ),
        ("platform.claude.com".into(), Route::Unresolved),
    ];
    let relay = ClaudeRelay::start(&routes).await.unwrap();
    routes.clear(); // A later system proxy change cannot affect this probe.
    let sock_task = tokio::spawn(async move {
        let (mut conn, _) = socks.accept().await.unwrap();
        let mut greeting = [0; 3];
        conn.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting, [5, 1, 0]);
        conn.write_all(&[5, 0]).await.unwrap();
        let mut head = [0; 5];
        conn.read_exact(&mut head).await.unwrap();
        assert_eq!(&head[..4], &[5, 1, 0, 3]);
        let mut name = vec![0; head[4] as usize];
        conn.read_exact(&mut name).await.unwrap();
        assert_eq!(name, b"unlisted.anthropic.com");
        assert_eq!(conn.read_u16().await.unwrap(), 443);
        conn.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 1])
            .await
            .unwrap();
        let mut dest = TcpStream::connect(echo_addr).await.unwrap();
        copy_bidirectional(&mut dest, &mut conn).await.unwrap();
    });
    let http_task = tokio::spawn(async move {
        let mut conn = BufReader::new(http.accept().await.unwrap().0);
        let head = header(&mut conn).await;
        assert!(head.starts_with("CONNECT claude.ai:443 HTTP/1.1\r\n"));
        assert!(head.contains(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            STANDARD.encode("alice:p@ss")
        )));
        conn.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
        let mut dest = TcpStream::connect(echo_addr).await.unwrap();
        copy_bidirectional(&mut dest, &mut conn).await.unwrap();
    });
    let url = Url::parse(relay.url()).unwrap();
    let auth = credential(&url);
    assert_eq!(
        request(&url, "CONNECT", "platform.claude.com:443", &auth, b"")
            .await
            .0,
        502
    );
    let mut clients = Vec::new();
    for target in ["unlisted.anthropic.com:443", "CLAUDE.AI:443"] {
        let (status, mut client) = request(&url, "CONNECT", target, &auth, b"\x16\x03\0\xff").await;
        assert_eq!(status, 200);
        let mut echo = [0; 4];
        client.read_exact(&mut echo).await.unwrap();
        assert_eq!(&echo, b"\x16\x03\0\xff");
        clients.push(client);
    }
    relay.stop().await;
    assert!(TcpStream::connect(("127.0.0.1", url.port().unwrap()))
        .await
        .is_err());
    for mut client in clients {
        assert_eq!(client.read(&mut [0; 1]).await.unwrap(), 0);
    }
    sock_task.await.unwrap();
    http_task.await.unwrap();
    echo_task.await.unwrap();
}
