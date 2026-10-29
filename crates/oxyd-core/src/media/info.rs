//! yt-dlp's description of a web page (`yt-dlp -J`) and the qualities offered for it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::RequestHeaders;

/// Highest resolution offered (8K).
const MAX_HEIGHT: u32 = 4320;

/// The parts of `yt-dlp -J` Oxyd uses. yt-dlp leaves out or nulls any field it does not know,
/// so everything is optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Info {
    pub id: Option<String>,
    pub title: Option<String>,
    pub duration: Option<f64>,
    pub uploader: Option<String>,
    pub extractor_key: Option<String>,
    pub webpage_url: Option<String>,
    pub is_live: Option<bool>,
    pub live_status: Option<String>,
    #[serde(rename = "_type")]
    pub kind: Option<String>,
    pub formats: Option<Vec<Format>>,
    /// Pages with a single file put its format at the top level instead of in `formats`.
    #[serde(flatten)]
    pub single: Format,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Format {
    pub format_id: Option<String>,
    pub format_note: Option<String>,
    pub ext: Option<String>,
    pub protocol: Option<String>,
    pub url: Option<String>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub height: Option<f64>,
    pub fps: Option<f64>,
    pub tbr: Option<f64>,
    pub abr: Option<f64>,
    pub filesize: Option<f64>,
    pub filesize_approx: Option<f64>,
    pub http_headers: Option<HashMap<String, String>>,
    pub dynamic_range: Option<String>,
    /// `mp4_dash`, `m4a_dash`, `webm_dash`: a DASH fragment that needs remuxing to play
    /// everywhere.
    pub container: Option<String>,
    /// `true`, `false` or `"maybe"`.
    pub has_drm: Option<serde_json::Value>,
    pub language_preference: Option<f64>,
    /// `a=b; Domain=...; Path=/` cookies a format needs.
    pub cookies: Option<String>,
}

impl Format {
    pub fn id(&self) -> &str {
        self.format_id.as_deref().unwrap_or("")
    }

    fn codec_set(c: &Option<String>) -> Option<bool> {
        match c.as_deref() {
            None => None,
            Some("none") => Some(false),
            Some(_) => Some(true),
        }
    }

    /// Has a picture. Unknown codecs (generic pages) count as a complete video.
    pub fn has_video(&self) -> bool {
        Self::codec_set(&self.vcodec).unwrap_or(Self::codec_set(&self.acodec) != Some(true))
    }

    pub fn has_audio(&self) -> bool {
        Self::codec_set(&self.acodec).unwrap_or(Self::codec_set(&self.vcodec) != Some(true))
    }

    fn usable(&self) -> bool {
        let drm = matches!(self.has_drm, Some(serde_json::Value::Bool(true)));
        let proto = self.protocol.as_deref().unwrap_or("");
        let ext = self.ext.as_deref().unwrap_or("");
        !drm && proto != "mhtml"
            && ext != "mhtml"
            && self.url.is_some()
            && (self.has_video() || self.has_audio())
    }

    /// A plain file Oxyd's engine can fetch with Range requests.
    pub fn is_plain_http(&self) -> bool {
        let proto = self.protocol.as_deref().unwrap_or("https");
        let url = self.url.as_deref().unwrap_or("");
        matches!(proto, "http" | "https")
            && (url.starts_with("https://") || url.starts_with("http://"))
    }

    pub fn is_dash_fragment(&self) -> bool {
        self.container
            .as_deref()
            .is_some_and(|c| c.ends_with("_dash"))
    }

    pub fn height(&self) -> u32 {
        self.height.map_or(0, |h| h.max(0.0) as u32)
    }

    fn hdr(&self) -> bool {
        self.dynamic_range
            .as_deref()
            .is_some_and(|d| !d.eq_ignore_ascii_case("sdr"))
    }

    /// Exact size when the site says it, else yt-dlp's estimate, else from the bitrate.
    pub fn size(&self, duration: Option<f64>) -> Option<u64> {
        self.filesize
            .or(self.filesize_approx)
            .or_else(|| Some(self.tbr? * 125.0 * duration?))
            .filter(|s| *s > 0.0)
            .map(|s| s as u64)
    }

    /// Request headers yt-dlp says this format's URL needs (the URL is often bound to its
    /// User-Agent), plus its cookies.
    pub fn request_headers(&self) -> RequestHeaders {
        let mut h = RequestHeaders::default();
        for (k, v) in self.http_headers.iter().flatten() {
            match k.to_ascii_lowercase().as_str() {
                "user-agent" => h.user_agent = Some(v.clone()),
                "referer" => h.referer = Some(v.clone()),
                "cookie" => h.cookie = Some(v.clone()),
                // Byte offsets must match the raw body.
                "accept-encoding" => {}
                _ => h.extra.push((k.clone(), v.clone())),
            }
        }
        if let Some(c) = self.cookies.as_deref().and_then(cookie_header) {
            h.cookie = Some(match h.cookie.take() {
                Some(old) => format!("{old}; {c}"),
                None => c,
            });
        }
        h
    }
}

/// `a=b; Domain=x; Path=/; c=d` (yt-dlp's cookie list) as a `Cookie` header value.
fn cookie_header(s: &str) -> Option<String> {
    const ATTRS: [&str; 8] = [
        "domain", "path", "expires", "max-age", "secure", "httponly", "samesite", "version",
    ];
    let pairs: Vec<&str> = s
        .split(';')
        .map(str::trim)
        .filter(|p| {
            let name = p
                .split('=')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            p.contains('=') && !ATTRS.contains(&name.as_str())
        })
        .collect();
    (!pairs.is_empty()).then(|| pairs.join("; "))
}

impl Info {
    pub fn parse(json: &[u8]) -> crate::Result<Info> {
        let info: Info = serde_json::from_slice(json)?;
        if info.kind.as_deref() == Some("playlist") {
            return Err(crate::Error::Video(
                "this page is a playlist; open a single video".into(),
            ));
        }
        Ok(info)
    }

    pub fn is_live(&self) -> bool {
        self.is_live == Some(true)
            || matches!(
                self.live_status.as_deref(),
                Some("is_live" | "is_upcoming" | "post_live")
            )
    }

    /// Every format, including the single top-level one.
    pub fn all_formats(&self) -> Vec<&Format> {
        match &self.formats {
            Some(f) if !f.is_empty() => f.iter().collect(),
            _ if self.single.url.is_some() => vec![&self.single],
            _ => vec![],
        }
    }

    pub fn format(&self, id: &str) -> Option<&Format> {
        self.all_formats().into_iter().find(|f| f.id() == id)
    }
}

/// Video with sound, or only its sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChoiceKind {
    Video,
    Audio,
}

/// One entry of the quality list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// `137+140`, `18`, or `mp3:140` (that audio converted to MP3).
    pub id: String,
    pub kind: ChoiceKind,
    /// `1080p60`, `MP3`, ...
    pub label: String,
    /// `MP4 · H.264`
    pub detail: String,
    pub size: Option<u64>,
    /// Extension of the file it makes.
    pub container: String,
    pub height: Option<u32>,
}

impl Choice {
    /// The yt-dlp format ids and whether the sound is converted to MP3.
    pub fn parse_id(id: &str) -> (&str, bool) {
        match id.strip_prefix("mp3:") {
            Some(rest) => (rest, true),
            None => (id, false),
        }
    }
}

fn codec_rank(vcodec: Option<&str>) -> u8 {
    let c = vcodec.unwrap_or("").to_ascii_lowercase();
    // H.264 plays everywhere; AV1 and VP9 are the only choice on YouTube above 1080p.
    if c.starts_with("avc") || c.starts_with("h264") {
        0
    } else if c.starts_with("av01") {
        1
    } else if c.starts_with("vp09") || c.starts_with("vp9") {
        2
    } else if c.starts_with("hev") || c.starts_with("hvc") {
        3
    } else if c.is_empty() {
        1
    } else {
        4
    }
}

fn codec_label(vcodec: Option<&str>) -> Option<&'static str> {
    let c = vcodec.unwrap_or("").to_ascii_lowercase();
    Some(match codec_rank(Some(&c)) {
        _ if c.is_empty() || c == "none" => return None,
        0 => "H.264",
        1 => "AV1",
        2 => "VP9",
        3 => "HEVC",
        _ => return None,
    })
}

fn proto_rank(f: &Format) -> u8 {
    match f.protocol.as_deref().unwrap_or("https") {
        "https" | "http" => 0,
        p if p.starts_with("m3u8") => 1,
        _ => 2,
    }
}

fn audio_ext_rank(f: &Format, video_ext: &str) -> u8 {
    let ext = f.ext.as_deref().unwrap_or("");
    let same_family = match video_ext {
        "webm" => ext == "webm",
        _ => matches!(ext, "m4a" | "mp4"),
    };
    u8::from(!same_family)
}

/// Best sound to go with a video in `video_ext`.
fn best_audio<'a>(formats: &[&'a Format], video_ext: &str) -> Option<&'a Format> {
    formats
        .iter()
        .copied()
        .filter(|f| f.has_audio() && !f.has_video())
        .min_by(|a, b| {
            let key = |f: &Format| {
                (
                    audio_ext_rank(f, video_ext),
                    proto_rank(f),
                    // The original track before dubs.
                    -(f.language_preference.unwrap_or(0.0) as i64),
                    -(f.abr.or(f.tbr).unwrap_or(0.0) as i64),
                )
            };
            key(a).cmp(&key(b))
        })
}

fn container_of(video: &Format, audio: Option<&Format>) -> String {
    let vext = video.ext.as_deref().unwrap_or("mp4");
    let Some(a) = audio else {
        return match vext {
            "mp4" | "webm" | "mkv" | "mov" | "flv" | "3gp" => vext.to_string(),
            _ => "mp4".into(),
        };
    };
    let aext = a.ext.as_deref().unwrap_or("");
    match (vext, aext) {
        ("mp4", "m4a" | "mp4") => "mp4".into(),
        ("webm", "webm") => "webm".into(),
        _ => "mkv".into(),
    }
}

/// The qualities to offer for `info`: one video per height (best first), then audio only.
pub fn choices(info: &Info) -> Vec<Choice> {
    let formats: Vec<&Format> = info
        .all_formats()
        .into_iter()
        .filter(|f| f.usable())
        .collect();
    let dur = info.duration;
    let mut by_height: Vec<(u32, &Format)> = Vec::new();
    for f in formats.iter().copied().filter(|f| f.has_video()) {
        let h = f.height();
        if h > MAX_HEIGHT {
            continue;
        }
        let key = |f: &Format| {
            (
                f.hdr(),
                codec_rank(f.vcodec.as_deref()),
                // Plain files go through Oxyd's own engine, HLS through yt-dlp.
                proto_rank(f),
                // A file that already has its sound needs no merging.
                !f.has_audio(),
                -(f.fps.unwrap_or(0.0) as i64),
                -(f.tbr.unwrap_or(0.0) as i64),
            )
        };
        match by_height.iter_mut().find(|(height, _)| *height == h) {
            Some(slot) if key(f) < key(slot.1) => slot.1 = f,
            Some(_) => {}
            None => by_height.push((h, f)),
        }
    }
    by_height.sort_by_key(|(h, _)| std::cmp::Reverse(*h));

    let mut out = Vec::new();
    for (h, v) in by_height {
        let vext = v.ext.as_deref().unwrap_or("mp4");
        let audio = if v.has_audio() {
            None
        } else {
            best_audio(&formats, vext)
        };
        // A picture without any sound available is still worth offering (GIF-like clips).
        let container = container_of(v, audio);
        let id = match audio {
            Some(a) => format!("{}+{}", v.id(), a.id()),
            None => v.id().to_string(),
        };
        let fps = v.fps.unwrap_or(0.0).round() as u32;
        let mut label = if h > 0 {
            format!("{h}p")
        } else {
            v.format_note.clone().unwrap_or_else(|| "Video".into())
        };
        if h > 0 && fps > 30 {
            label.push_str(&fps.to_string());
        }
        if v.hdr() {
            label.push_str(" HDR");
        }
        let mut detail = container.to_uppercase();
        if let Some(c) = codec_label(v.vcodec.as_deref()) {
            detail.push_str(" · ");
            detail.push_str(c);
        }
        let size = match (v.size(dur), audio.map(|a| a.size(dur))) {
            (Some(a), Some(Some(b))) => Some(a + b),
            (s, None) => s,
            _ => None,
        };
        out.push(Choice {
            id,
            kind: ChoiceKind::Video,
            label,
            detail,
            size,
            container,
            height: (h > 0).then_some(h),
        });
    }

    // Sound only: AAC as it comes (no conversion), and MP3 for everything else.
    let audio_only: Vec<&Format> = formats.iter().copied().filter(|f| !f.has_video()).collect();
    if let Some(a) = best_audio(&audio_only, "mp4") {
        if matches!(a.ext.as_deref(), Some("m4a" | "mp4")) {
            out.push(Choice {
                id: a.id().to_string(),
                kind: ChoiceKind::Audio,
                label: "M4A".into(),
                detail: abr_detail("AAC", a),
                size: a.size(dur),
                container: "m4a".into(),
                height: None,
            });
        }
    }
    let mp3_source = best_audio(&audio_only, "webm").or_else(|| {
        // No separate sound: take it from the smallest complete video.
        formats
            .iter()
            .copied()
            .filter(|f| f.has_video() && f.has_audio())
            .min_by_key(|f| (proto_rank(f), f.height()))
    });
    if let Some(a) = mp3_source {
        out.push(Choice {
            id: format!("mp3:{}", a.id()),
            kind: ChoiceKind::Audio,
            label: "MP3".into(),
            detail: "MP3 · 192 kbps".into(),
            size: dur.map(|d| (d * 192.0 * 125.0) as u64),
            container: "mp3".into(),
            height: None,
        });
    }
    out
}

fn abr_detail(name: &str, f: &Format) -> String {
    match f.abr.or(f.tbr) {
        Some(k) if k > 0.0 => format!("{name} · {} kbps", k.round() as u32),
        _ => name.to_string(),
    }
}

/// The choice `id` stands for in `info`: itself when its formats are still there, else the
/// same kind of choice at the closest height not above `height` (some sites renumber their
/// formats between visits). `best` is the first video.
pub fn find_choice(info: &Info, id: &str, height: Option<u32>) -> Option<Choice> {
    let list = choices(info);
    if id == "best" {
        return list.into_iter().find(|c| c.kind == ChoiceKind::Video);
    }
    if let Some(c) = list.iter().find(|c| c.id == id) {
        return Some(c.clone());
    }
    let (formats, mp3) = Choice::parse_id(id);
    if formats.split('+').all(|f| info.format(f).is_some()) {
        // Formats the list does not show by themselves (picked elsewhere): keep them.
        let first = info.format(formats.split('+').next()?)?;
        let audio = formats.split('+').nth(1).and_then(|a| info.format(a));
        return Some(Choice {
            id: id.to_string(),
            kind: if mp3 || !first.has_video() {
                ChoiceKind::Audio
            } else {
                ChoiceKind::Video
            },
            label: String::new(),
            detail: String::new(),
            size: None,
            container: if mp3 {
                "mp3".into()
            } else {
                container_of(first, audio)
            },
            height: (first.height() > 0).then_some(first.height()),
        });
    }
    if mp3 {
        return list.into_iter().find(|c| c.container == "mp3");
    }
    match height {
        Some(h) => list
            .iter()
            .filter(|c| c.kind == ChoiceKind::Video && c.height.unwrap_or(0) <= h)
            .max_by_key(|c| c.height)
            .or_else(|| list.iter().find(|c| c.kind == ChoiceKind::Video))
            .cloned(),
        None => list.into_iter().find(|c| c.kind == ChoiceKind::Audio),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed `yt-dlp -J` of a YouTube video.
    pub(crate) const YOUTUBE: &str = r#"{
        "id": "abc", "title": "A video", "duration": 100, "extractor_key": "Youtube",
        "formats": [
            {"format_id": "sb0", "ext": "mhtml", "protocol": "mhtml", "vcodec": "none", "acodec": "none", "url": "https://i.ytimg.com/sb"},
            {"format_id": "139", "ext": "m4a", "protocol": "https", "vcodec": "none", "acodec": "mp4a.40.5", "abr": 48, "filesize": 600000, "url": "https://r1.googlevideo.com/139", "container": "m4a_dash"},
            {"format_id": "140", "ext": "m4a", "protocol": "https", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129, "filesize": 1600000, "url": "https://r1.googlevideo.com/140", "container": "m4a_dash"},
            {"format_id": "251", "ext": "webm", "protocol": "https", "vcodec": "none", "acodec": "opus", "abr": 135, "filesize": 1700000, "url": "https://r1.googlevideo.com/251"},
            {"format_id": "18", "ext": "mp4", "protocol": "https", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "height": 360, "fps": 30, "tbr": 500, "url": "https://r1.googlevideo.com/18"},
            {"format_id": "134", "ext": "mp4", "protocol": "https", "vcodec": "avc1.4d401e", "acodec": "none", "height": 360, "fps": 30, "tbr": 300, "filesize": 3000000, "url": "https://r1.googlevideo.com/134"},
            {"format_id": "137", "ext": "mp4", "protocol": "https", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "fps": 30, "tbr": 4000, "filesize": 40000000, "url": "https://r1.googlevideo.com/137", "http_headers": {"User-Agent": "UA", "Accept-Encoding": "gzip"}},
            {"format_id": "248", "ext": "webm", "protocol": "https", "vcodec": "vp9", "acodec": "none", "height": 1080, "fps": 30, "tbr": 3000, "url": "https://r1.googlevideo.com/248"},
            {"format_id": "313", "ext": "webm", "protocol": "https", "vcodec": "vp9", "acodec": "none", "height": 2160, "fps": 60, "tbr": 20000, "filesize": 200000000, "url": "https://r1.googlevideo.com/313"},
            {"format_id": "96", "ext": "mp4", "protocol": "m3u8_native", "vcodec": "avc1.640028", "acodec": "mp4a.40.2", "height": 1080, "fps": 30, "tbr": 4500, "url": "https://manifest.googlevideo.com/96.m3u8"}
        ]
    }"#;

    #[test]
    fn offers_one_quality_per_height_and_audio() {
        let info = Info::parse(YOUTUBE.as_bytes()).unwrap();
        let list = choices(&info);
        let ids: Vec<&str> = list.iter().map(|c| c.id.as_str()).collect();
        // 4K only as VP9 → WebM with Opus; 1080p as H.264 + AAC in MP4 (plain HTTP before
        // the HLS one); 360p as the file that already has sound.
        assert_eq!(ids, ["313+251", "137+140", "18", "140", "mp3:251"]);
        assert_eq!(list[0].label, "2160p60");
        assert_eq!(list[0].container, "webm");
        assert_eq!(list[1].container, "mp4");
        assert_eq!(list[1].detail, "MP4 · H.264");
        assert_eq!(list[1].size, Some(41_600_000));
        assert_eq!(list[3].detail, "AAC · 129 kbps");
        assert_eq!(list[3].kind, ChoiceKind::Audio);
    }

    #[test]
    fn finds_a_choice_again() {
        let info = Info::parse(YOUTUBE.as_bytes()).unwrap();
        assert_eq!(find_choice(&info, "best", None).unwrap().id, "313+251");
        assert_eq!(
            find_choice(&info, "137+140", Some(1080)).unwrap().id,
            "137+140"
        );
        // Renumbered formats: the closest height not above the old one.
        assert_eq!(
            find_choice(&info, "999+998", Some(1440)).unwrap().id,
            "137+140"
        );
        assert_eq!(find_choice(&info, "mp3:777", None).unwrap().id, "mp3:251");
        let manual = find_choice(&info, "248+251", Some(1080)).unwrap();
        assert_eq!(manual.container, "webm");
    }

    #[test]
    fn headers_from_formats() {
        let info = Info::parse(YOUTUBE.as_bytes()).unwrap();
        let h = info.format("137").unwrap().request_headers();
        assert_eq!(h.user_agent.as_deref(), Some("UA"));
        assert!(h.extra.is_empty(), "Accept-Encoding dropped: {:?}", h.extra);
        let f = Format {
            cookies: Some("a=1; Domain=.x.com; Path=/; Secure; b=2".into()),
            ..Default::default()
        };
        assert_eq!(f.request_headers().cookie.as_deref(), Some("a=1; b=2"));
    }

    #[test]
    fn single_file_pages_and_playlists() {
        let info = Info::parse(
            br#"{"title": "clip", "url": "https://cdn.example.com/v.mp4", "ext": "mp4", "format_id": "0", "protocol": "https"}"#,
        )
        .unwrap();
        let list = choices(&info);
        assert_eq!(list[0].id, "0");
        assert_eq!(list[0].label, "Video");
        assert_eq!(list[0].container, "mp4");
        assert_eq!(list.last().unwrap().id, "mp3:0");
        assert!(Info::parse(br#"{"_type": "playlist", "entries": []}"#).is_err());
    }
}
