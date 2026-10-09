//! F18-FR1 / FR2: `MyStyle/Meetings/calendar-links.txt`, one read-only iCalendar link per line.
//! The file is the user's (Meetings never writes it, INV16) and each URL is a secret, handled
//! like an API key (INV15): it stays in [`Link`], which never prints it, and only its id, name
//! and elided form leave the core.
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The links file, relative to the storage root.
pub const LINKS_FILE: &str = "MyStyle/Meetings/calendar-links.txt";
/// A links file is a few lines; bigger is not one (as a key file, `keyfile.rs`).
const MAX_FILE_BYTES: u64 = 64 * 1024;
/// More links than this is not a calendar list.
pub const MAX_LINKS: usize = 32;
/// A link longer than this is not one.
const MAX_URL: usize = 2048;
const MAX_LABEL: usize = 80;

/// One calendar link. The URL is private: `Debug` redacts it and only [`Link::expose`] (the
/// fetch, the scrubber) reads it.
#[derive(Clone, PartialEq, Eq)]
pub struct Link {
    /// The first 8 hex characters of the URL's SHA-256 (F18-FR2): the link's name across JNI,
    /// in the cache file name and in event keys.
    pub id: String,
    /// `name = url`'s name, when the line gave one.
    pub label: Option<String>,
    /// The URL's host, the last fallback for the calendar's name.
    pub host: String,
    /// "https://calendar.google.com/…​.ics": scheme and host, "…", the last 4 characters.
    pub elided: String,
    url: String,
}

impl Link {
    /// The URL itself: every call site is a place it can leak (INV15).
    pub fn expose(&self) -> &str {
        &self.url
    }

    /// What the scrubber removes for this link: the whole URL, and its path and query (where
    /// the secret is), never just the host.
    pub fn secrets(&self) -> Vec<String> {
        let rest = self
            .url
            .strip_prefix("https://")
            .and_then(|r| r.find('/').map(|i| r[i..].to_owned()));
        let mut v = vec![self.url.clone()];
        if let Some(r) = rest.filter(|r| r.len() > 1) {
            if let Some((_, q)) = r.split_once('?').filter(|(_, q)| !q.is_empty()) {
                v.push(q.to_owned());
            }
            v.push(r);
        }
        v
    }
}

impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Link")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("elided", &self.elided)
            .field("url", &"[redacted]")
            .finish()
    }
}

/// A line that isn't a link, by line number (1-based). Never carries the line's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub line: usize,
    pub reason: &'static str,
}

/// The links in the file, and the lines that were not links.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Links {
    pub links: Vec<Link>,
    pub skipped: Vec<Skipped>,
    /// Why the file couldn't be read (not "missing": a missing file is no links).
    pub unreadable: Option<String>,
}

/// F18-FR1: parse the file's text. Blank and `#` lines are skipped; `name = url` labels a link;
/// only `https://` is accepted and `webcal://` reads as `https://`. A URL listed twice is kept
/// once.
pub fn parse(text: &str) -> Links {
    let mut out = Links::default();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let skip = |reason| Skipped {
            line: i + 1,
            reason,
        };
        let (label, url) = match split_label(line) {
            Some((l, u)) => (Some(l), u),
            None => (None, line),
        };
        match link_of(label, url) {
            Ok(l) if out.links.iter().any(|o| o.id == l.id) => {}
            Ok(_) if out.links.len() >= MAX_LINKS => out.skipped.push(skip("too many links")),
            Ok(l) => out.links.push(l),
            Err(reason) => out.skipped.push(skip(reason)),
        }
    }
    out
}

/// `name = url`: the `=` before the URL's scheme (a URL's own query may hold `=`).
fn split_label(line: &str) -> Option<(&str, &str)> {
    let scheme = line.find("://")?;
    let eq = line[..scheme].rfind('=')?;
    let (name, url) = (line[..eq].trim(), line[eq + 1..].trim());
    Some((name, url))
}

fn link_of(label: Option<&str>, url: &str) -> Result<Link, &'static str> {
    let url = normalise(url)?;
    let host = host_of(&url).ok_or("not a link")?;
    let label = label
        .map(|l| {
            l.chars()
                .filter(|c| !c.is_control())
                .take(MAX_LABEL)
                .collect::<String>()
        })
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty());
    Ok(Link {
        id: link_id(&url),
        elided: elide(&url, &host),
        host,
        label,
        url,
    })
}

/// `webcal://` → `https://`; anything but `https://` is refused (F18-FR1).
fn normalise(url: &str) -> Result<String, &'static str> {
    if url.len() > MAX_URL {
        return Err("link too long");
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("not a link");
    }
    let lower = url.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("webcal://") || lower.starts_with("webcals://") {
        &url[url.find("://").expect("scheme checked") + 3..]
    } else if lower.starts_with("http://") {
        return Err("only https:// links are read");
    } else {
        return Err("not a link");
    };
    Ok(format!("https://{rest}"))
}

/// The host of an `https://` URL, lowercased, without user info or port.
fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let auth = rest.split(['/', '?', '#']).next()?;
    let host = auth.rsplit('@').next()?;
    let host = host.split(':').next()?.to_ascii_lowercase();
    let ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    ok.then_some(host)
}

/// F18-FR2's id: the first 8 hex characters of the SHA-256 of the URL.
pub fn link_id(url: &str) -> String {
    Sha256::digest(url.as_bytes())
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// F18-FR2's elided form: scheme and host, "…", and the last 4 characters.
fn elide(url: &str, host: &str) -> String {
    let n = url.chars().count();
    let tail: String = url.chars().skip(n.saturating_sub(4)).collect();
    format!("https://{host}/…{tail}")
}

/// Where the links file is under `storage_root`.
pub fn path_in(storage_root: &Path) -> PathBuf {
    storage_root.join(LINKS_FILE)
}

/// The file's change stamp: re-read only when it moves (F18-FR1).
pub fn stamp(path: &Path) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// Read the links file. Missing is no links; an error names the I/O kind only, never the path
/// or a line (as `keyfile::read_capped`).
pub fn load(path: &Path) -> Links {
    match crate::atomic::read_capped(path, MAX_FILE_BYTES) {
        Ok(bytes) => parse(&String::from_utf8_lossy(&bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Links::default(),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Links {
            unreadable: Some("file too large".into()),
            ..Links::default()
        },
        Err(e) => Links {
            unreadable: Some(e.kind().to_string()),
            ..Links::default()
        },
    }
}

/// A file's change stamp, `None` when it is missing.
type Stamp = Option<(SystemTime, u64)>;

/// The links file, re-read only when its stamp changes.
#[derive(Debug, Default)]
pub struct LinksCache {
    seen: std::sync::Mutex<Option<(Stamp, Links)>>,
}

impl LinksCache {
    pub fn get(&self, path: &Path) -> Links {
        let now = stamp(path);
        let mut g = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*g {
            Some((s, l)) if *s == now && now.is_some() => l.clone(),
            _ => {
                let l = load(path);
                *g = Some((now, l.clone()));
                l
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "https://calendar.google.com/calendar/ical/me%40example.com/private-0123456789abcdef/basic.ics";

    #[test]
    fn lines_labels_and_schemes() {
        let text = format!(
            "\u{feff}# my calendars\n\n  Work = {G}  \nwebcal://outlook.office365.com/owa/calendar/abc/def/calendar.ics?x=1\nhttp://insecure.example/cal.ics\nnot a link\n{G}\nftp://x/y\n"
        );
        let l = parse(&text);
        assert_eq!(l.links.len(), 2, "{l:?}");
        assert_eq!(l.links[0].label.as_deref(), Some("Work"));
        assert_eq!(l.links[0].host, "calendar.google.com");
        assert_eq!(l.links[0].expose(), G);
        assert_eq!(l.links[0].elided, "https://calendar.google.com/….ics");
        let o = &l.links[1];
        assert!(o.expose().starts_with("https://outlook.office365.com/"));
        assert_eq!(o.label, None);
        assert_eq!(o.elided, "https://outlook.office365.com/…?x=1");
        let reasons: Vec<_> = l.skipped.iter().map(|s| (s.line, s.reason)).collect();
        assert_eq!(
            reasons,
            [
                (5, "only https:// links are read"),
                (6, "not a link"),
                (8, "not a link")
            ]
        );
    }

    #[test]
    fn ids_are_the_sha256_prefix() {
        // sha256("abc") = ba7816bf…
        assert_eq!(link_id("abc"), "ba7816bf");
        let l = parse(G);
        assert_eq!(l.links[0].id, link_id(G));
        assert_eq!(l.links[0].id.len(), 8);
        // webcal and https name the same link.
        let w = parse(&G.replacen("https", "webcal", 1));
        assert_eq!(w.links[0].id, l.links[0].id);
    }

    #[test]
    fn a_link_never_prints() {
        let l = &parse(G).links[0];
        let dbg = format!("{l:?}");
        assert!(!dbg.contains("private-0123"), "{dbg}");
        let s = l.secrets();
        assert!(s.contains(&G.to_owned()));
        assert!(s.iter().any(|x| x.starts_with("/calendar/ical/")));
        let q = &parse("https://h.example/p/cal.ics?token=SECRETSECRET").links[0];
        assert!(q.secrets().contains(&"token=SECRETSECRET".to_owned()));
        let bare = &parse("https://h.example").links[0];
        assert_eq!(bare.secrets(), ["https://h.example"]);
    }

    #[test]
    fn odd_hosts_and_limits() {
        assert!(parse("https://user@Host.Example:8443/x").links[0].host == "host.example");
        assert!(parse("https:///nohost").links.is_empty());
        assert!(parse("https://bad_host/x").links.is_empty());
        let long = format!("https://h.example/{}", "a".repeat(3000));
        assert_eq!(parse(&long).skipped[0].reason, "link too long");
        let many: String = (0..40)
            .map(|i| format!("https://h{i}.example/c.ics\n"))
            .collect();
        let l = parse(&many);
        assert_eq!(l.links.len(), MAX_LINKS);
        assert_eq!(l.skipped.len(), 8);
        let lab = parse("  = https://h.example/c.ics").links[0].label.clone();
        assert_eq!(lab, None);
    }

    #[test]
    fn the_file_is_read_and_re_read_on_change() {
        let d = tempfile::tempdir().unwrap();
        let p = path_in(d.path());
        assert_eq!(load(&p), Links::default());
        let c = LinksCache::default();
        assert!(c.get(&p).links.is_empty());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "https://a.example/1.ics\n").unwrap();
        assert_eq!(c.get(&p).links.len(), 1);
        assert_eq!(c.get(&p).links.len(), 1, "cached");
        std::fs::write(&p, "https://a.example/1.ics\nhttps://b.example/2.ics\n").unwrap();
        assert_eq!(c.get(&p).links.len(), 2, "size changed");
        std::fs::write(&p, vec![b'a'; 70 * 1024]).unwrap();
        assert_eq!(load(&p).unreadable.as_deref(), Some("file too large"));
        let dir_as_file = d.path().join("dir");
        std::fs::create_dir_all(&dir_as_file).unwrap();
        assert!(load(&dir_as_file).unreadable.is_some());
    }
}
