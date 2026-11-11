//! BitTorrent through librqbit: magnet links and `.torrent` files.
//!
//! Oxyd downloads a torrent and stops: it uploads to other peers while it downloads, as
//! the protocol expects, but does not keep seeding afterwards.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use librqbit::api::TorrentIdOrHash;
use librqbit::dht::Id20;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ByteBufOwned, ManagedTorrent, Session,
    SessionOptions, TorrentStatsState, ValidatedTorrentMetaV1Info,
};

use crate::error::{Error, Result};
use crate::filename;

fn err(e: impl std::fmt::Display) -> Error {
    Error::Torrent(e.to_string())
}

/// Whether this system can open IPv6 sockets (some containers and hardened kernels cannot).
pub fn ipv6_available() -> bool {
    std::net::UdpSocket::bind((std::net::Ipv6Addr::UNSPECIFIED, 0)).is_ok()
}

/// What a torrent holds, known once its metadata arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub info_hash: String,
    /// Name of the single file, or of the folder holding several.
    pub name: String,
    pub total_size: u64,
    pub files: usize,
}

impl Meta {
    fn of(hash: Id20, info: &ValidatedTorrentMetaV1Info<ByteBufOwned>) -> Self {
        let sizes: Vec<u64> = info.iter_file_details().map(|f| f.len).collect();
        let name = info
            .name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "torrent".into());
        Meta {
            info_hash: hash.as_string(),
            name: filename::sanitize(&name),
            total_size: sizes.iter().sum(),
            files: sizes.len(),
        }
    }

    /// Where the content lies under `dir`: the file itself, or the torrent's folder.
    pub fn content_path(&self, dir: &Path) -> PathBuf {
        dir.join(&self.name)
    }
}

/// Live numbers of a torrent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
    /// Bytes per second.
    pub speed: u64,
    pub peers: u32,
    pub finished: bool,
    /// Files are being checked against their hashes (after adding existing data).
    pub checking: bool,
    pub error: Option<String>,
}

/// A torrent that was added to the session.
pub struct Torrent {
    session: Arc<Session>,
    handle: Arc<ManagedTorrent>,
}

impl Torrent {
    pub fn progress(&self) -> Progress {
        let stats = self.handle.stats();
        let live = stats.live.as_ref();
        Progress {
            downloaded: stats.progress_bytes,
            total: stats.total_bytes,
            speed: live
                .map(|l| (l.download_speed.mbps * 1024.0 * 1024.0).max(0.0) as u64)
                .unwrap_or(0),
            peers: live.map(|l| l.snapshot.peer_stats.live).unwrap_or(0),
            finished: stats.finished,
            checking: matches!(stats.state, TorrentStatsState::Initializing { .. }),
            error: stats.error,
        }
    }

    /// Take the torrent out of the session. The files stay on disk.
    pub async fn stop(self) {
        let id = TorrentIdOrHash::Hash(self.handle.info_hash());
        if let Err(e) = self.session.delete(id, false).await {
            tracing::debug!("removing torrent from the session: {e}");
        }
    }
}

/// The BitTorrent session of the app.
pub struct Engine {
    session: Arc<Session>,
}

impl Engine {
    /// A session that finds peers through DHT, trackers and the local network.
    pub async fn start(default_dir: PathBuf) -> Result<Self> {
        Self::with_options(
            default_dir,
            SessionOptions {
                client_name_and_version: Some(format!("Oxyd/{}", env!("CARGO_PKG_VERSION"))),
                ..Default::default()
            },
        )
        .await
    }

    /// A session that only talks to the peers it is told about (tests). With `listen` it
    /// accepts connections on a free local port.
    pub async fn start_offline(default_dir: PathBuf, listen: bool) -> Result<Self> {
        Self::with_options(
            default_dir,
            SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                persistence: None,
                listen: listen.then(|| librqbit::ListenerOptions {
                    listen_addr: (std::net::Ipv4Addr::LOCALHOST, 0).into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
    }

    /// Make a torrent of `path` and serve it. Returns the `.torrent` file's bytes and the
    /// address other sessions reach this one at (tests).
    #[doc(hidden)]
    pub async fn seed(&self, path: &Path) -> Result<(Vec<u8>, std::net::SocketAddr)> {
        let (torrent, _handle) = self
            .session
            .create_and_serve_torrent(path, librqbit::CreateTorrentOptions::default())
            .await
            .map_err(err)?;
        let port = self
            .session
            .listen_addr()
            .ok_or_else(|| Error::Torrent("the session does not listen".into()))?
            .port();
        Ok((
            torrent.as_bytes().map_err(err)?.to_vec(),
            (std::net::Ipv4Addr::LOCALHOST, port).into(),
        ))
    }

    async fn with_options(default_dir: PathBuf, options: SessionOptions) -> Result<Self> {
        let session = Session::new_with_opts(default_dir, options)
            .await
            .map_err(|e| {
                // librqbit opens an IPv6 socket for trackers whatever the options say.
                if !ipv6_available() {
                    Error::Torrent(
                        "BitTorrent needs IPv6 sockets, which are switched off on this system"
                            .into(),
                    )
                } else {
                    err(format!("{e:#}"))
                }
            })?;
        Ok(Engine { session })
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    /// The source of a download's `url`: a magnet or http link, or the path of a file.
    fn source(url: &str) -> Result<AddTorrent<'static>> {
        if url.starts_with("magnet:") || url.starts_with("http://") || url.starts_with("https://") {
            return Ok(AddTorrent::from_url(url.to_string()));
        }
        let path = url.strip_prefix("file://").unwrap_or(url);
        let bytes = std::fs::read(path)
            .map_err(|e| Error::Torrent(format!("cannot read the torrent file: {e}")))?;
        Ok(AddTorrent::from_bytes(bytes))
    }

    /// Read what the torrent holds without downloading it. A magnet link asks other peers
    /// for the metadata first, which can take a while.
    pub async fn inspect(
        &self,
        url: &str,
        peers: Option<Vec<std::net::SocketAddr>>,
    ) -> Result<Meta> {
        let options = AddTorrentOptions {
            list_only: true,
            initial_peers: peers,
            ..Default::default()
        };
        let resp = self
            .session
            .add_torrent(Self::source(url)?, Some(options))
            .await
            .map_err(err)?;
        match resp {
            AddTorrentResponse::ListOnly(list) => Ok(Meta::of(list.info_hash, &list.info)),
            AddTorrentResponse::AlreadyManaged(_, h) | AddTorrentResponse::Added(_, h) => {
                let hash = h.info_hash();
                h.with_metadata(|m| Meta::of(hash, &m.info)).map_err(err)
            }
        }
    }

    /// Start (or continue) downloading into `dir`. Data that is already there is checked
    /// and kept. `peers` are tried in addition to the ones the network finds.
    pub async fn add(
        &self,
        url: &str,
        meta: &Meta,
        dir: &Path,
        peers: Option<Vec<std::net::SocketAddr>>,
    ) -> Result<Torrent> {
        // One file goes straight into `dir`; several get the torrent's folder.
        let folder = if meta.files >= 2 {
            meta.content_path(dir)
        } else {
            dir.to_path_buf()
        };
        let options = AddTorrentOptions {
            output_folder: Some(folder.to_string_lossy().into_owned()),
            overwrite: true,
            initial_peers: peers,
            ..Default::default()
        };
        let handle = self
            .session
            .add_torrent(Self::source(url)?, Some(options))
            .await
            .map_err(err)?
            .into_handle()
            .ok_or_else(|| Error::Torrent("the torrent was not added".into()))?;
        Ok(Torrent {
            session: self.session.clone(),
            handle,
        })
    }

    /// Download limit of the whole session in bytes/second; `None` lifts it.
    pub fn set_limit(&self, bytes_per_sec: Option<u64>) {
        let rate = bytes_per_sec.and_then(|v| NonZeroU32::new(v.min(u32::MAX as u64) as u32));
        self.session.ratelimits.set_download_bps(rate);
    }
}
