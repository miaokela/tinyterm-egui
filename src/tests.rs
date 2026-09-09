//! Unit tests for the non-GUI logic: crypto round-trips, storage CRUD,
//! terminal key encoding, output parsers and path helpers.

use crate::models::*;
use crate::remote_fs;
use crate::storage::Db;
use crate::term;

fn temp_db(tag: &str) -> (Db, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "tinyterm-egui-test-{tag}-{}-{}",
        std::process::id(),
        crate::models::now_millis()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tinyterm.db");
    (Db::open(&path).unwrap(), dir)
}

#[test]
fn crypto_round_trip() {
    let (db, dir) = temp_db("crypto");
    let secret = "p@ssw0rd-中文-🔑";
    let envelope = crate::crypto::encrypt_secret(&db.path, secret).unwrap();
    assert!(crate::crypto::is_encrypted_secret(&envelope));
    // `ttenc:v1:` contributes two colons, then three field separators.
    assert_eq!(envelope.matches(':').count(), 5);
    let back = crate::crypto::decrypt_secret(&db.path, &envelope).unwrap();
    assert_eq!(back, secret);
    // Legacy passthrough
    assert_eq!(
        crate::crypto::decrypt_secret(&db.path, "plain").unwrap(),
        "plain"
    );
    // Empty stays empty
    assert_eq!(crate::crypto::encrypt_secret(&db.path, "").unwrap(), "");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn legacy_password_obfuscation() {
    let raw = "secret123";
    let encoded = encode_password(raw);
    assert_ne!(encoded, raw);
    assert_eq!(decode_password(&encoded), raw);
}

#[test]
fn storage_bookmark_crud() {
    let (db, dir) = temp_db("bookmark");
    let mut bookmark = Bookmark {
        title: "prod".into(),
        host: "10.0.0.1".into(),
        username: "root".into(),
        auth_type: "password".into(),
        ..Default::default()
    };
    let created = db.create_bookmark(&bookmark, Some("hunter2")).unwrap();
    assert_eq!(created.title, "prod");

    // Secrets are never returned by `list_bookmarks`.
    let listed = db.list_bookmarks().unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].password.is_none());

    // But the SSH layer can read them back.
    let full = db.bookmark_with_secrets(&bookmark.id).unwrap().unwrap();
    let password = db
        .resolve_password_secret(full.password.as_deref(), full.password_encrypted)
        .unwrap();
    assert_eq!(password.as_deref(), Some("hunter2"));

    bookmark.title = "prod-2".into();
    let updated = db.update_bookmark(&bookmark, None).unwrap();
    assert_eq!(updated.title, "prod-2");

    // Clearing the password.
    db.update_bookmark(&bookmark, Some("")).unwrap();
    let full = db.bookmark_with_secrets(&bookmark.id).unwrap().unwrap();
    assert!(full.password.unwrap_or_default().is_empty());

    db.delete_bookmark(&bookmark.id).unwrap();
    assert!(db.list_bookmarks().unwrap().is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn storage_settings_and_trusted_keys() {
    let (db, dir) = temp_db("settings");
    let mut settings = db.get_settings().unwrap();
    assert_eq!(settings.font_size, 12);
    settings.font_size = 18;
    settings.cursor_blink = false;
    db.save_settings(&settings).unwrap();
    let reloaded = db.get_settings().unwrap();
    assert_eq!(reloaded.font_size, 18);
    assert!(!reloaded.cursor_blink);

    let key = TrustedHostKey {
        host: "example.com".into(),
        port: 22,
        key_type: "ssh-ed25519".into(),
        fingerprint: "SHA256:abc".into(),
        created_at: 1,
        updated_at: 1,
    };
    db.upsert_trusted_host_key(&key).unwrap();
    assert_eq!(
        db.get_trusted_host_key("example.com", 22)
            .unwrap()
            .unwrap()
            .fingerprint,
        "SHA256:abc"
    );
    db.delete_trusted_host_key("example.com", 22).unwrap();
    assert!(db.get_trusted_host_key("example.com", 22).unwrap().is_none());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn terminal_emulation_and_key_encoding() {
    let mut term = term::Terminal::new(24, 80, 1000);
    term.process(b"hello \x1b[31mworld\x1b[0m\r\n");
    let screen = term.screen();
    assert_eq!(screen.size(), (24, 80));
    let mut text = String::new();
    for row in screen.rows(0, 80) {
        text.push_str(&row);
        text.push('\n');
    }
    assert!(text.contains("hello world"));

    // Colour is applied to the "world" run.
    let cell = screen.cell(0, 6).unwrap();
    assert!(matches!(cell.fgcolor(), vt100::Color::Idx(1)));

    // Resize
    term.resize(30, 100);
    assert_eq!(term.size(), (30, 100));

    // Key encoding
    use egui::Key;
    assert_eq!(
        term::encode_key(Some(Key::Enter), "", false, false, false, false).unwrap(),
        "\r"
    );
    assert_eq!(
        term::encode_key(Some(Key::Backspace), "", false, false, false, false).unwrap(),
        "\x7f"
    );
    assert_eq!(
        term::encode_key(Some(Key::ArrowUp), "", false, false, false, false).unwrap(),
        "\x1b[A"
    );
    assert_eq!(
        term::encode_key(Some(Key::ArrowUp), "", false, false, false, true).unwrap(),
        "\x1bOA"
    );
    assert_eq!(
        term::encode_key(Some(Key::C), "", true, false, false, false).unwrap(),
        "\x03"
    );
    assert_eq!(
        term::encode_key(Some(Key::Tab), "", false, false, true, false).unwrap(),
        "\x1b[Z"
    );
    assert_eq!(
        term::encode_key(Some(Key::Delete), "", false, false, false, false).unwrap(),
        "\x1b[3~"
    );
}

#[test]
fn path_helpers() {
    assert_eq!(remote_fs::join_path("/a", "b"), "/a/b");
    assert_eq!(remote_fs::join_path("/a/", "b"), "/a/b");
    assert_eq!(remote_fs::join_path("", "b"), "b");
    assert_eq!(remote_fs::parent_of("/a/b/c"), "/a/b");
    assert_eq!(remote_fs::parent_of("/a"), "/");
    assert_eq!(remote_fs::basename("/a/b/c"), "c");
    assert_eq!(remote_fs::normalize_remote_path("/a/b///"), "/a/b");
    assert_eq!(remote_fs::normalize_remote_path("/"), "/");
    assert_eq!(remote_fs::shell_quote("a'b"), "'a'\"'\"'b'");
    assert_eq!(remote_fs::map_stage_progress(100, 50, 20, 60), 50);
    assert_eq!(remote_fs::map_stage_progress(0, 0, 20, 60), 20);
    assert_eq!(remote_fs::map_stage_progress(100, 200, 20, 60), 80);
}

#[test]
fn output_parsers() {
    let procs = crate::ui::system_info::parse_process_output(
        "  123  12.5  nginx  /usr/sbin/nginx -g\n456 3.0 [kworker]\nnot a row\n",
    );
    assert_eq!(procs.len(), 2);
    assert_eq!(procs[0], vec!["123", "12.5", "nginx", "/usr/sbin/nginx -g"]);
    assert_eq!(procs[1][3], "-");

    let disks = crate::ui::system_info::parse_disk_output(
        "Filesystem      Size  Used Avail Use% Mounted on\n/dev/disk1s1   500G  200G  300G  40% /\n",
    );
    assert_eq!(disks.len(), 1);
    assert_eq!(disks[0][4], "40%");

    let history = crate::ui::system_info::parse_history(
        ": 1700000000:0;ls -la\n  12  git status\nplain-command\n",
    );
    assert_eq!(history, vec!["ls -la", "git status", "plain-command"]);
}

#[test]
fn model_helpers() {
    assert_eq!(human_size(0), "0 B");
    assert_eq!(human_size(1023), "1023 B");
    assert_eq!(human_size(1024), "1.00 KB");
    assert_eq!(human_size(1536), "1.50 KB");
    assert_eq!(human_size(10 * 1024 * 1024), "10.0 MB");

    let c = parse_hex_color("#2f7dff").unwrap();
    assert_eq!((c.r(), c.g(), c.b()), (0x2f, 0x7d, 0xff));
    assert_eq!(color_to_hex(c), "#2f7dff");
    assert!(parse_hex_color("nope").is_none());

    let t = time_from_unix(1_700_000_000);
    assert_eq!(t.0, 2023);
}

#[test]
fn settings_normalization_migrates_legacy_defaults() {
    let mut s = Settings::default();
    s.font_size = 14;
    let normalized = crate::storage::normalize_settings(s);
    assert_eq!(normalized.font_size, 12);

    let mut s = Settings::default();
    s.font_size = 20;
    let normalized = crate::storage::normalize_settings(s);
    assert_eq!(normalized.font_size, 20);
}

#[test]
fn transfer_progress_percent() {
    let mut t = TransferProgress {
        id: "x".into(),
        file_name: "f".into(),
        direction: TransferDirection::Upload,
        total: 200,
        transferred: 50,
        status: TransferStatus::Transferring,
        error: None,
        target_path: None,
        conflict_path: None,
        conflict_is_dir: false,
        session_id: None,
        group_id: None,
        created_at_ms: 0,
    };
    assert_eq!(t.percent(), 25.0);
    t.transferred = 200;
    assert_eq!(t.percent(), 100.0);
    t.total = 0;
    t.status = TransferStatus::Done;
    assert_eq!(t.percent(), 100.0);
}

// ── End-to-end SSH test against an in-process russh server ───────────────────

mod ssh_server {
    use russh::server::{Auth, Msg, Server as _, Session};
    use russh::{Channel, ChannelId};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[derive(Clone)]
    pub struct TestServer;

    impl russh::server::Server for TestServer {
        type Handler = TestHandler;
        fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> TestHandler {
            TestHandler
        }
    }

    #[derive(Clone)]
    pub struct TestHandler;

    fn respond(session: &mut Session, channel: ChannelId, body: &str) -> Result<(), russh::Error> {
        session.channel_success(channel)?;
        session.data(channel, body.as_bytes().to_vec())?;
        session.exit_status_request(channel, 0)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }

    impl russh::server::Handler for TestHandler {
        type Error = russh::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            password: &str,
        ) -> Result<Auth, Self::Error> {
            Ok(if password == "hunter2" {
                Auth::Accept
            } else {
                Auth::reject()
            })
        }

        async fn channel_open_session(
            &mut self,
            _channel: Channel<Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            Ok(())
        }

        async fn pty_request(
            &mut self,
            channel: ChannelId,
            _term: &str,
            _cols: u32,
            _rows: u32,
            _pw: u32,
            _ph: u32,
            _modes: &[(russh::Pty, u32)],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            Ok(())
        }

        async fn shell_request(
            &mut self,
            channel: ChannelId,
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            session.data(channel, b"shell-ready\r\n".to_vec())?;
            Ok(())
        }

        async fn exec_request(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            let cmd = String::from_utf8_lossy(data).to_string();
            if cmd.contains("$HOME") {
                respond(session, channel, "/home/tester")
            } else if let Some(rest) = cmd.strip_prefix("echo ") {
                respond(session, channel, &format!("{rest}\r\n"))
            } else if cmd.contains("pwd") {
                respond(session, channel, "/home/tester\r\n")
            } else {
                respond(session, channel, "ok\r\n")
            }
        }

        async fn data(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            // Echo everything back, like a PTY would.
            session.data(channel, data.to_vec())?;
            Ok(())
        }
    }

    /// Start the test server on an ephemeral port; returns the port.
    pub async fn start() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut config = russh::server::Config::default();
        config.keys = vec![russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh::keys::Algorithm::Ed25519,
        )
        .unwrap()];
        config.inactivity_timeout = Some(std::time::Duration::from_secs(60));
        config.auth_rejection_time = std::time::Duration::from_millis(10);
        let config = Arc::new(config);
        let mut server = TestServer;
        tokio::spawn(async move {
            let _ = server.run_on_socket(config, &listener).await;
        });
        port
    }
}

#[test]
fn ssh_end_to_end() {
    use crate::ssh::{self, ConnectError, WriteCmd};
    use russh::ChannelMsg;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let port = ssh_server::start().await;
        let (db, dir) = temp_db("ssh");

        let bookmark = Bookmark {
            title: "test".into(),
            host: "127.0.0.1".into(),
            port,
            username: "tester".into(),
            auth_type: "password".into(),
            ..Default::default()
        };
        let created = db.create_bookmark(&bookmark, Some("hunter2")).unwrap();
        let auth = ssh::resolve_auth(&db, &created, None, None).unwrap();
        assert_eq!(auth.username, "tester");
        assert_eq!(auth.password.as_deref(), Some("hunter2"));

        // 1) First contact must be refused with a host-key prompt.
        let prompt = match ssh::connect(&auth, None, std::time::Duration::from_secs(5)).await {
            Err(ConnectError::HostKey(p)) => *p,
            Err(other) => panic!("expected a host-key prompt, got: {other}"),
            Ok(_) => panic!("expected a host-key prompt, connection succeeded"),
        };
        assert_eq!(prompt.reason, "unknown");
        assert_eq!(prompt.key_type, "ssh-ed25519");
        assert!(prompt.fingerprint.starts_with("SHA256:"));

        // 2) After trusting the fingerprint the handshake succeeds.
        let trusted = TrustedHostKey {
            host: prompt.host.clone(),
            port: prompt.port,
            key_type: prompt.key_type.clone(),
            fingerprint: prompt.fingerprint.clone(),
            created_at: 0,
            updated_at: 0,
        };
        let mut conn = ssh::connect(&auth, Some(trusted.clone()), std::time::Duration::from_secs(5))
            .await
            .expect("handshake after trusting the key");

        // 3) Wrong password is rejected.
        let bad = ssh::ResolvedAuth {
            password: Some("nope".into()),
            ..auth.clone()
        };
        let mut bad_conn = ssh::connect(&auth, Some(trusted.clone()), std::time::Duration::from_secs(5))
            .await
            .unwrap();
        let err = ssh::authenticate(&mut bad_conn.handle, &bad, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Authentication failed"));

        // 4) Correct password succeeds.
        ssh::authenticate(&mut conn.handle, &auth, None)
            .await
            .expect("password auth");

        // 5) exec + remote_home + cwd
        let out = ssh::exec_string(&conn.handle, "echo hello").await.unwrap();
        assert!(out.contains("hello"));
        assert_eq!(ssh::remote_home(&conn.handle).await.unwrap(), "/home/tester");
        assert_eq!(ssh::remote_cwd(&conn.handle).await.unwrap(), "/home/tester");

        // 6) PTY shell: data is echoed back through the reader half.
        let (mut read_half, tx) = ssh::open_shell(&conn.handle, "xterm-256color", 80, 24)
            .await
            .expect("open shell");
        tx.send(WriteCmd::Data(b"ping\n".to_vec())).unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut buf = Vec::new();
        loop {
            match tokio::time::timeout_at(deadline, read_half.wait()).await {
                Ok(Some(ChannelMsg::Data { data })) => {
                    buf.extend_from_slice(&data);
                    if String::from_utf8_lossy(&buf).contains("ping") {
                        break;
                    }
                }
                Ok(Some(ChannelMsg::Eof)) | Ok(Some(ChannelMsg::Close)) => break,
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&buf);
        assert!(text.contains("shell-ready"), "shell banner missing: {text:?}");
        assert!(text.contains("ping"), "echo missing: {text:?}");

        // 7) SFTP subsystem negotiation fails against this stub server, but the
        //    failure must surface as an error rather than a hang.
        let sftp = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            ssh::open_sftp(&conn.handle),
        )
        .await;
        assert!(sftp.is_ok(), "open_sftp hung");

        std::fs::remove_dir_all(dir).ok();
    });
}

#[test]
fn local_tar_round_trip() {
    let root = std::env::temp_dir().join(format!(
        "tinyterm-egui-tar-{}-{}",
        std::process::id(),
        crate::models::now_millis()
    ));
    let src = root.join("payload");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), b"alpha").unwrap();
    std::fs::write(src.join("sub/b.txt"), b"beta").unwrap();

    let tar_path = root.join("pack.tar");
    let size = remote_fs::pack_local_dir(
        &src.to_string_lossy(),
        &tar_path.to_string_lossy(),
    )
    .unwrap();
    assert!(size > 0);
    assert!(tar_path.exists());

    let dest = root.join("out");
    std::fs::create_dir_all(&dest).unwrap();
    let count =
        remote_fs::unpack_local_dir(&tar_path.to_string_lossy(), &dest.to_string_lossy(), false)
            .unwrap();
    assert!(count >= 3, "expected at least 3 entries, got {count}");
    assert_eq!(
        std::fs::read_to_string(dest.join("payload/a.txt")).unwrap(),
        "alpha"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("payload/sub/b.txt")).unwrap(),
        "beta"
    );

    // Non-overwrite mode skips existing entries instead of failing.
    let count =
        remote_fs::unpack_local_dir(&tar_path.to_string_lossy(), &dest.to_string_lossy(), false)
            .unwrap();
    assert!(count >= 3);

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn local_delete_guards() {
    assert!(crate::local_fs::delete("/").is_err());
    let home = crate::local_fs::home_dir();
    assert!(crate::local_fs::delete(&home.to_string_lossy()).is_err());

    let dir = std::env::temp_dir().join(format!(
        "tinyterm-egui-del-{}-{}",
        std::process::id(),
        crate::models::now_millis()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x"), b"1").unwrap();
    crate::local_fs::delete(&dir.to_string_lossy()).unwrap();
    assert!(!dir.exists());
}

#[test]
fn ui_text_metrics_are_script_independent() {
    // The CJK face is installed as the *primary* proportional font on purpose:
    // with a Latin primary + CJK fallback, each script gets its own row height
    // and labels end up looking vertically off-centre.
    let has_cjk = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists());
    if !has_cjk {
        eprintln!("skipping: no CJK system font installed");
        return;
    }

    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "Menlo", 12.0);
    let font = crate::theme::f_sm();
    let measure = |text: &str| {
        ctx.fonts_mut(|f| {
            f.layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .y
        })
    };
    let latin = measure("ABC");
    let cjk = measure("中文测试");
    let mixed = measure("ABC 中文");
    assert!(latin > 0.0 && cjk > 0.0, "empty layout: {latin} {cjk}");
    assert!(
        (latin - cjk).abs() < 0.51,
        "row heights differ: latin={latin} cjk={cjk}"
    );
    assert!(
        (latin - mixed).abs() < 0.51,
        "row heights differ: latin={latin} mixed={mixed}"
    );
}
