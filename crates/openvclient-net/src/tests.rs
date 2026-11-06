//! Live tests against the docker test network (see CLAUDE.md). Skipped when
//! the proxies are not running.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

async fn ping_redis(local: SocketAddr) -> String {
    let mut s = TcpStream::connect(local).await.unwrap();
    s.write_all(b"PING\r\n").await.unwrap();
    let mut buf = [0u8; 16];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf)).await.unwrap().unwrap();
    String::from_utf8_lossy(&buf[..n]).trim().to_string()
}

fn up(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), Duration::from_millis(300)).is_ok()
}

#[tokio::test]
async fn socks5_and_http_proxies() {
    if !up(51080) || !up(53128) {
        eprintln!("skipping: proxies not running");
        return;
    }
    let t = Tunnel::open(ProxySpec::Socks5 { host: "127.0.0.1".into(), port: 51080, auth: Some(("puser".into(), "ppass".into())) }, "redis", 6379).await.unwrap();
    assert_eq!(ping_redis(t.local).await, "+PONG");
    // Several concurrent connections over one forwarder.
    let (a, b) = tokio::join!(ping_redis(t.local), ping_redis(t.local));
    assert_eq!((a.as_str(), b.as_str()), ("+PONG", "+PONG"));
    let bad = Tunnel::open(ProxySpec::Socks5 { host: "127.0.0.1".into(), port: 51080, auth: Some(("puser".into(), "wrong".into())) }, "redis", 6379).await;
    assert!(bad.is_err());

    let t = Tunnel::open(ProxySpec::Http { host: "127.0.0.1".into(), port: 53128, auth: None }, "redis", 6379).await.unwrap();
    assert_eq!(ping_redis(t.local).await, "+PONG");
    let unreachable = Tunnel::open(ProxySpec::Http { host: "127.0.0.1".into(), port: 53128, auth: None }, "no-such-host.invalid", 1).await;
    assert!(unreachable.is_err());
}

#[tokio::test]
async fn ssh_tunnel_password_key_and_pinning() {
    if !up(52222) {
        eprintln!("skipping: ssh server not running");
        return;
    }
    let spec = |auth, fp: Option<String>| ProxySpec::Ssh { host: "127.0.0.1".into(), port: 52222, user: "tunnel".into(), auth, known_fingerprint: fp, timeout: Duration::from_secs(10) };
    let t = Tunnel::open(spec(SshAuth::Password("tunnelpw".into()), None), "redis", 6379).await.unwrap();
    assert_eq!(ping_redis(t.local).await, "+PONG");
    let fp = t.fingerprint.clone().unwrap();
    assert!(fp.starts_with("SHA256:"), "{fp}");

    // Pinned key accepted; a different pin is refused.
    Tunnel::open(spec(SshAuth::Password("tunnelpw".into()), Some(fp)), "redis", 6379).await.unwrap();
    let e = Tunnel::open(spec(SshAuth::Password("tunnelpw".into()), Some("SHA256:not-the-key".into())), "redis", 6379).await.err().unwrap();
    assert!(e.message.contains("host key"), "{e}");

    let e = Tunnel::open(spec(SshAuth::Password("nope".into()), None), "redis", 6379).await.err().unwrap();
    assert_eq!(e.kind, openvclient_core::ErrorKind::Auth, "{e}");

    if let Ok(key) = std::env::var("OPENVCLIENT_TEST_SSH_KEY") {
        let t = Tunnel::open(spec(SshAuth::Key { path: key.into(), passphrase: None }, None), "redis", 6379).await.unwrap();
        assert_eq!(ping_redis(t.local).await, "+PONG");
    }
}
