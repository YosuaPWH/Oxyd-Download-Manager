//! Simulates the browser: starts `oxyd-bridge` and sends it a message.
//!
//! ```text
//! cargo run -p oxyd-bridge --example browser_sim -- ping
//! cargo run -p oxyd-bridge --example browser_sim -- https://example.com/file.zip [filename]
//! ```
//!
//! `OXYD_SIM_COOKIES` and `OXYD_SIM_SIZE` set the cookies and size the extension would send.

use std::process::{Command, Stdio};

use oxyd_protocol::{read_message, write_message, AddDownload, Message};

fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap_or_else(|| "ping".into());
    let msg = if target == "ping" {
        Message::Ping
    } else {
        Message::AddDownload(AddDownload {
            id: format!("sim-{}", std::process::id()),
            url: target,
            filename: args.next(),
            source: Some("browser_sim".into()),
            cookies: std::env::var("OXYD_SIM_COOKIES").ok(),
            size: std::env::var("OXYD_SIM_SIZE")
                .ok()
                .and_then(|s| s.parse().ok()),
            user_agent: Some("browser_sim".into()),
            ..Default::default()
        })
    };
    let exe = std::env::current_exe()
        .expect("current exe")
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("oxyd-bridge.exe"))
        .expect("target dir");
    let mut child = Command::new(&exe)
        .arg("chrome-extension://browser-sim/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| {
            panic!(
                "cannot start {}: {e} (build oxyd-bridge first)",
                exe.display()
            )
        });
    let mut stdin = child.stdin.take().expect("stdin");
    write_message(&mut stdin, &msg).expect("write");
    drop(stdin);
    let out = child.wait_with_output().expect("bridge output");
    let reply = read_message(&mut &out.stdout[..]).expect("reply");
    println!("{reply:#?}");
}
