//! A small FTP client: enough to size a file and fetch it from an offset.
//!
//! Plain FTP in passive mode over one data connection (`EPSV`, then `PASV`), binary type,
//! resumed with `REST`. No FTPS: servers that require TLS answer the login with an error
//! that is reported as it is.

use std::net::SocketAddr;
use std::time::Duration;

use percent_encoding::percent_decode_str;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use crate::error::{Error, Result};
use crate::filename;
use crate::probe::ProbeInfo;

const TIMEOUT: Duration = Duration::from_secs(20);

/// Where an `ftp://` link points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Path on the server, percent-decoded.
    pub path: String,
}

impl Target {
    pub fn parse(url: &str) -> Result<Self> {
        let u = reqwest::Url::parse(url).map_err(|e| Error::InvalidUrl(format!("{url}: {e}")))?;
        if u.scheme() != "ftp" {
            return Err(Error::InvalidUrl(format!(
                "not an ftp link: {}",
                u.scheme()
            )));
        }
        let decode = |s: &str| percent_decode_str(s).decode_utf8_lossy().into_owned();
        let host = u
            .host_str()
            .ok_or_else(|| Error::InvalidUrl("no host".into()))?
            .trim_matches(['[', ']'])
            .to_string();
        let path = decode(u.path());
        if path.is_empty() || path.ends_with('/') {
            return Err(Error::InvalidUrl(
                "the link names a folder, not a file".into(),
            ));
        }
        Ok(Target {
            host,
            port: u.port().unwrap_or(21),
            user: Some(decode(u.username()))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "anonymous".into()),
            password: u
                .password()
                .map(decode)
                .unwrap_or_else(|| "anonymous@".into()),
            path,
        })
    }

    /// File name of the link, sanitized.
    pub fn file_name(&self) -> Option<String> {
        self.path
            .rsplit('/')
            .next()
            .filter(|n| !n.is_empty())
            .map(filename::sanitize)
    }
}

fn net(e: impl std::fmt::Display) -> Error {
    Error::Network(e.to_string())
}

/// Map a refused command to the error the UI knows.
fn refused(code: u16, text: &str) -> Error {
    match code {
        // Not logged in, or the file is not there / not readable.
        530 | 532 => Error::LinkExpired(403),
        550 | 553 => Error::Http {
            status: 404,
            message: text.to_string(),
        },
        // Temporary: too many users, try later.
        421 | 425 | 426 | 450 | 451 | 452 => Error::Network(format!("ftp {code}: {text}")),
        _ => Error::InvalidState(format!("ftp {code}: {text}")),
    }
}

/// The control connection, logged in and in binary mode.
pub struct Session {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    peer: SocketAddr,
}

impl Session {
    pub async fn connect(t: &Target) -> Result<Self> {
        let stream = tokio::time::timeout(TIMEOUT, TcpStream::connect((t.host.as_str(), t.port)))
            .await
            .map_err(|_| net("connecting timed out"))?
            .map_err(net)?;
        let peer = stream.peer_addr().map_err(net)?;
        let (r, writer) = stream.into_split();
        let mut s = Session {
            reader: BufReader::new(r),
            writer,
            peer,
        };
        s.expect(&[220]).await?;
        let (code, text) = s.cmd(&format!("USER {}", t.user)).await?;
        match code {
            230 => {}
            331 | 332 => {
                s.cmd_ok(&format!("PASS {}", t.password), &[230, 202])
                    .await?;
            }
            _ => return Err(refused(code, &text)),
        }
        s.cmd_ok("TYPE I", &[200]).await?;
        Ok(s)
    }

    /// Read one reply; a multi-line reply ends with the line `NNN text`.
    async fn reply(&mut self) -> Result<(u16, String)> {
        let mut first: Option<u16> = None;
        loop {
            let mut line = String::new();
            let n = tokio::time::timeout(TIMEOUT, self.reader.read_line(&mut line))
                .await
                .map_err(|_| net("the server stopped answering"))?
                .map_err(net)?;
            if n == 0 {
                return Err(net("the server closed the connection"));
            }
            let line = line.trim_end();
            let code = line.get(..3).and_then(|c| c.parse::<u16>().ok());
            let last = line.as_bytes().get(3) != Some(&b'-');
            match (first, code) {
                (None, Some(c)) if last => return Ok((c, line[3..].trim().to_string())),
                (None, Some(c)) => first = Some(c),
                (Some(f), Some(c)) if c == f && last => {
                    return Ok((c, line[3..].trim().to_string()))
                }
                (None, None) => return Err(net(format!("unexpected answer: {line}"))),
                _ => {}
            }
        }
    }

    async fn expect(&mut self, ok: &[u16]) -> Result<String> {
        let (code, text) = self.reply().await?;
        if ok.contains(&code) {
            Ok(text)
        } else {
            Err(refused(code, &text))
        }
    }

    async fn cmd(&mut self, line: &str) -> Result<(u16, String)> {
        self.writer
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .map_err(net)?;
        self.reply().await
    }

    async fn cmd_ok(&mut self, line: &str, ok: &[u16]) -> Result<String> {
        let (code, text) = self.cmd(line).await?;
        if ok.contains(&code) {
            Ok(text)
        } else {
            Err(refused(code, &text))
        }
    }

    /// Size of `path` in bytes, when the server tells.
    pub async fn size(&mut self, path: &str) -> Result<Option<u64>> {
        let (code, text) = self.cmd(&format!("SIZE {path}")).await?;
        match code {
            213 => Ok(text.trim().parse().ok()),
            550 => Err(refused(code, &text)),
            // SIZE is an extension; without it the download still works.
            _ => Ok(None),
        }
    }

    /// Whether the server restarts transfers at an offset.
    pub async fn can_restart(&mut self) -> Result<bool> {
        let ok = self.cmd("REST 1").await?.0 == 350;
        if ok {
            self.cmd("REST 0").await?;
        }
        Ok(ok)
    }

    /// Open a passive data connection. The address is always the control connection's:
    /// servers behind NAT announce addresses that cannot be reached.
    async fn passive(&mut self) -> Result<TcpStream> {
        let (code, text) = self.cmd("EPSV").await?;
        let port = if code == 229 {
            parse_epsv(&text)
        } else {
            let text = self.cmd_ok("PASV", &[227]).await?;
            parse_pasv(&text)
        }
        .ok_or_else(|| net("the server sent no data port"))?;
        let addr = SocketAddr::new(self.peer.ip(), port);
        tokio::time::timeout(TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| net("the data connection timed out"))?
            .map_err(net)
    }

    /// Start sending `path` from byte `offset`. Read the returned stream to its end, then
    /// call [`Session::finish`].
    pub async fn retrieve(&mut self, path: &str, offset: u64) -> Result<TcpStream> {
        let data = self.passive().await?;
        if offset > 0 {
            self.cmd_ok(&format!("REST {offset}"), &[350]).await?;
        }
        self.cmd_ok(&format!("RETR {path}"), &[125, 150]).await?;
        Ok(data)
    }

    /// Wait for the server to confirm that the whole file was sent.
    pub async fn finish(&mut self) -> Result<()> {
        self.expect(&[226, 250]).await.map(|_| ())
    }
}

/// `Entering Extended Passive Mode (|||50000|)`
fn parse_epsv(text: &str) -> Option<u16> {
    let inner = text.split('(').nth(1)?.split(')').next()?;
    let delim = inner.chars().next()?;
    inner.split(delim).nth(3)?.parse().ok()
}

/// `Entering Passive Mode (127,0,0,1,195,80)`
fn parse_pasv(text: &str) -> Option<u16> {
    let inner = text.split('(').nth(1)?.split(')').next()?;
    let n: Vec<u16> = inner
        .split(',')
        .map(|p| p.trim().parse())
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    (n.len() == 6 && n[4] < 256 && n[5] < 256).then(|| n[4] * 256 + n[5])
}

/// Size and name of the file behind an `ftp://` link, for the "New download" dialog.
pub async fn probe(url: &str) -> Result<ProbeInfo> {
    let t = Target::parse(url)?;
    let mut s = Session::connect(&t).await?;
    let size = s.size(&t.path).await?;
    let restart = s.can_restart().await?;
    Ok(ProbeInfo {
        final_url: url.to_string(),
        size,
        accept_ranges: restart,
        etag: None,
        last_modified: None,
        filename: t.file_name(),
        mime: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        let t = Target::parse("ftp://bob:p%40ss@files.example.org:2121/pub/My%20File.iso").unwrap();
        assert_eq!(t.host, "files.example.org");
        assert_eq!(t.port, 2121);
        assert_eq!((t.user.as_str(), t.password.as_str()), ("bob", "p@ss"));
        assert_eq!(t.path, "/pub/My File.iso");
        assert_eq!(t.file_name().as_deref(), Some("My File.iso"));

        let t = Target::parse("ftp://ftp.example.org/a.zip").unwrap();
        assert_eq!((t.port, t.user.as_str()), (21, "anonymous"));
        assert!(Target::parse("ftp://ftp.example.org/pub/").is_err());
        assert!(Target::parse("https://example.org/a.zip").is_err());
    }

    #[test]
    fn parses_passive_answers() {
        assert_eq!(
            parse_epsv("Entering Extended Passive Mode (|||50000|)"),
            Some(50000)
        );
        assert_eq!(
            parse_pasv("Entering Passive Mode (10,0,0,5,195,80)."),
            Some(50000)
        );
        assert_eq!(parse_pasv("Entering Passive Mode (10,0,0,5,195)"), None);
        assert_eq!(parse_epsv("nothing"), None);
    }
}
