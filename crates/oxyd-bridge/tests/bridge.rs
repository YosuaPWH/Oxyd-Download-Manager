//! Drive the bridge binary like a browser does and check the replies.

use std::io::Write;
use std::process::{Command, Stdio};

use oxyd_protocol::{
    read_message, write_frame, write_message, Ack, AckStatus, AddDownload, Message,
};

fn bridge(pipe: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_oxyd-bridge"));
    c.env("OXYD_PIPE", pipe)
        .env("OXYD_APP", "Z:\\does-not-exist\\Oxyd.exe")
        .arg("chrome-extension://test/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    c
}

fn add(id: &str) -> Message {
    Message::AddDownload(AddDownload {
        id: id.into(),
        url: "https://example.com/file.zip".into(),
        filename: Some("file.zip".into()),
        ..Default::default()
    })
}

#[tokio::test]
async fn forwards_messages_to_the_app() {
    let pipe = format!(r"\\.\pipe\oxyd-bridge-test-{}", std::process::id());
    let server = oxyd_protocol::pipe::bind(&pipe).unwrap();
    tokio::spawn(oxyd_protocol::pipe::serve(
        pipe.clone(),
        server,
        |msg| async move {
            match msg {
                Message::Ping => Message::Pong {
                    app_version: "9.9.9".into(),
                },
                Message::AddDownload(a) => Ack::new(a.id, AckStatus::Accepted, None),
                _ => Ack::new("", AckStatus::Rejected, None),
            }
        },
    ));

    let out = tokio::task::spawn_blocking(move || {
        let mut child = bridge(&pipe).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        write_message(&mut stdin, &Message::Ping).unwrap();
        write_message(&mut stdin, &add("dl-1")).unwrap();
        // Invalid JSON keeps the bridge alive and reports the id when possible.
        write_frame(
            &mut stdin,
            br#"{"v":1,"type":"add_download","id":"bad","url":"ftp://x"}"#,
        )
        .unwrap();
        write_frame(&mut stdin, b"{oops").unwrap();
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        output.stdout
    })
    .await
    .unwrap();

    let mut r = &out[..];
    assert_eq!(
        read_message(&mut r).unwrap(),
        Some(Message::Pong {
            app_version: "9.9.9".into()
        })
    );
    assert_eq!(
        read_message(&mut r).unwrap(),
        Some(Ack::new("dl-1", AckStatus::Accepted, None))
    );
    for expected_id in ["bad", ""] {
        match read_message(&mut r).unwrap() {
            Some(Message::Ack(a)) => {
                assert_eq!(a.status, AckStatus::Error);
                assert_eq!(a.id, expected_id);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(
        read_message(&mut r).unwrap(),
        None,
        "stdout must hold frames only"
    );
}

#[test]
fn reports_error_when_app_is_unreachable() {
    let mut child = bridge(r"\\.\pipe\oxyd-nobody-listens").spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    write_message(&mut stdin, &Message::Ping).unwrap();
    write_message(&mut stdin, &add("dl-2")).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let mut r = &output.stdout[..];
    for id in ["", "dl-2"] {
        match read_message(&mut r).unwrap() {
            Some(Message::Ack(a)) => {
                assert_eq!(a.status, AckStatus::Error);
                assert_eq!(a.id, id);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn rejects_oversized_frames() {
    let mut child = bridge(r"\\.\pipe\oxyd-nobody-listens").spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(&((2 * 1024 * 1024) as u32).to_le_bytes())
        .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let mut r = &output.stdout[..];
    assert!(matches!(
        read_message(&mut r).unwrap(),
        Some(Message::Ack(Ack {
            status: AckStatus::Error,
            ..
        }))
    ));
}
