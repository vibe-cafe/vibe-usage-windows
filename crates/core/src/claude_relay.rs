//! A single Claude quota probe's authenticated loopback CONNECT proxy.
//! Forwarding is pinned to the routes captured before the probe started.
use crate::proxy_route::Route;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use http::Uri;
use hyper_util::client::legacy::connect::{
    proxy::{SocksV5, Tunnel},
    HttpConnector,
};
use hyper_util::rt::TokioIo;
use percent_encoding::percent_decode_str;
use std::{io, sync::Arc};
use tokio::io::{
    copy_bidirectional, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::{JoinHandle, JoinSet};
use tower_service::Service;
use url::Url;

pub struct ClaudeRelay {
    url: String,
    // Dropping this sender also closes the listener on probe cancellation.
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl ClaudeRelay {
    pub async fn start(routes: &[(String, Route)]) -> io::Result<Self> {
        let mut secret = [0; 24];
        getrandom::fill(&mut secret)
            .map_err(|_| io::Error::other("relay credential unavailable"))?;
        let credential = format!("vibe-usage:{}", URL_SAFE_NO_PAD.encode(secret));
        let authorization = format!("Basic {}", STANDARD.encode(&credential));
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let url = format!("http://{credential}@{}", listener.local_addr()?);
        let (stop, mut stopped) = oneshot::channel();
        let state = Arc::new(Forwarding {
            authorization,
            routes: routes.to_vec(),
        });
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    Some(_) = connections.join_next(), if !connections.is_empty() => {},
                    incoming = listener.accept() => {
                        let stream = match incoming {
                            Ok((stream, _)) => stream,
                            Err(error) => {
                                log::warn!("Claude relay failed to accept a connection: {:?}", error.kind());
                                continue;
                            }
                        };
                        let state = state.clone();
                        connections.spawn(async move { state.serve(stream).await });
                    }
                }
            }
            drop(listener);
            connections.shutdown().await;
        });
        Ok(Self { url, stop, task })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

struct Forwarding {
    authorization: String,
    routes: Vec<(String, Route)>,
}

impl Forwarding {
    fn route(&self, host: &str) -> &Route {
        self.routes
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(host))
            // Another first-party host uses the captured SOCKS route.
            .or_else(|| {
                self.routes
                    .iter()
                    .find(|(_, r)| matches!(r, Route::Socks(_)))
            })
            .map(|(_, r)| r)
            .unwrap_or(&Route::Unresolved)
    }

    fn target(&self, header: &str) -> Result<String, &'static str> {
        let mut lines = header.split("\r\n");
        let mut request = lines.next().unwrap_or("").split_ascii_whitespace();
        if request.next() != Some("CONNECT") {
            return Err("405 Method Not Allowed");
        }
        if !lines.any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("Proxy-Authorization")
                    && value.trim() == self.authorization
            })
        }) {
            return Err("407 Proxy Authentication Required");
        }
        let host = request
            .next()
            .and_then(|target| target.strip_suffix(":443"))
            .ok_or("403 Forbidden")?
            .to_ascii_lowercase();
        let domain = ["anthropic.com", "claude.com", "claude.ai"]
            .iter()
            .any(|domain| {
                host == *domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            });
        if !domain
            || !host.split('.').all(|label| {
                !label.is_empty()
                    && label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
        {
            return Err("403 Forbidden");
        }
        Ok(host)
    }

    async fn serve(&self, stream: TcpStream) -> io::Result<()> {
        // Keep any tunnel bytes already in the same packet in this reader.
        let mut client = BufReader::new(stream);
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            if bytes.len() == 8192 {
                return Ok(());
            }
            bytes.push(client.read_u8().await?);
        }
        let target = self.target(std::str::from_utf8(&bytes).unwrap_or(""));
        let result = match target {
            Ok(host) => connect(self.route(&host), &host)
                .await
                .map_err(|_| "502 Bad Gateway"),
            Err(status) => Err(status),
        };
        match result {
            Ok(mut upstream) => {
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await?;
                copy_bidirectional(&mut client, &mut upstream).await?;
            }
            Err(status) => {
                let challenge = if status.starts_with("407") {
                    "Proxy-Authenticate: Basic realm=\"Claude quota\"\r\n"
                } else {
                    ""
                };
                client
                    .write_all(
                        format!("HTTP/1.1 {status}\r\n{challenge}Content-Length: 0\r\n\r\n")
                            .as_bytes(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}
type Error = Box<dyn std::error::Error + Send + Sync>;

async fn connect(route: &Route, host: &str) -> Result<Box<dyn Stream>, Error> {
    let target: Uri = format!("https://{host}:443").parse()?;
    let proxy = match route {
        Route::Direct => return Ok(Box::new(TcpStream::connect((host, 443)).await?)),
        Route::Unresolved => return Err("unresolved route".into()),
        Route::Socks(endpoint) => Url::parse(&format!("socks5://{endpoint}"))?,
        Route::Http(url) => Url::parse(url)?,
    };
    let proxy_host = proxy.host_str().ok_or("missing proxy host")?;
    let port = proxy.port_or_known_default().ok_or("missing proxy port")?;
    let user = percent_decode_str(proxy.username()).decode_utf8()?;
    let password = percent_decode_str(proxy.password().unwrap_or("")).decode_utf8()?;
    let auth = !user.is_empty() || !password.is_empty();
    if matches!(route, Route::Socks(_)) {
        let endpoint = format!("http://{proxy_host}:{port}").parse()?;
        let mut socks = SocksV5::new(endpoint, HttpConnector::new()).local_dns(false);
        if auth {
            socks = socks.with_auth(user.into_owned(), password.into_owned());
        }
        return Ok(Box::new(socks.call(target).await?.into_inner()));
    }
    let endpoint = format!("{}://{proxy_host}:{port}", proxy.scheme()).parse()?;
    let authorization =
        auth.then(|| format!("Basic {}", STANDARD.encode(format!("{user}:{password}"))));
    // TLS, if requested, ends only at the user's HTTPS proxy. The inner
    // Claude-to-Anthropic session stays opaque even on this path.
    if proxy.scheme() == "https" {
        let mut tunnel = Tunnel::new(endpoint, hyper_tls::HttpsConnector::new());
        if let Some(value) = authorization {
            tunnel = tunnel.with_auth(value.parse()?);
        }
        Ok(Box::new(TokioIo::new(tunnel.call(target).await?)))
    } else if proxy.scheme() == "http" {
        let mut tunnel = Tunnel::new(endpoint, HttpConnector::new());
        if let Some(value) = authorization {
            tunnel = tunnel.with_auth(value.parse()?);
        }
        Ok(Box::new(tunnel.call(target).await?.into_inner()))
    } else {
        Err("unsupported proxy".into())
    }
}
