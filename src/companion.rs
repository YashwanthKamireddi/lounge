//! A photo from the owner's phone, for the wallpaper or the profile picture. While the QR code
//! is on screen (and only then), a small HTTP server listens on the TV's own network address;
//! the code holds its address and a random token. The phone's page fits the photo itself
//! (any format its browser can show, iPhone HEIC included) and sends a JPEG; the TV decodes it
//! again, makes what it needs (see `photo`) and answers.
//!
//! Nothing runs before the code is shown or after it closes: the server thread blocks in
//! `accept` and is ended by a connection of its own (see `Drop`).
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::Duration,
};

/// What the photo becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Kind {
    Wallpaper,
    Avatar,
}

/// What a session reports to the loop.
#[derive(Debug)]
pub enum Event {
    /// The phone's page was opened: someone scanned the code.
    Opened,
    /// A photo arrived and is being prepared.
    Receiving,
    /// Saved: the new wallpaper's id, or the new picture's file name in the avatars folder.
    Saved(Kind, String),
    Failed(String),
}

/// The largest upload taken: the page sends a fitted JPEG of a few MB at most.
const MAX_BODY: usize = 16 * 1024 * 1024;
/// A phone that stops sending mid-request is dropped after this.
const STALL: Duration = Duration::from_secs(20);
const TOKEN_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// A running session. Dropping it stops the server.
pub struct Session {
    pub url: String,
    /// The QR code's modules, row by row, `size` × `size`.
    pub size: usize,
    pub modules: Vec<bool>,
    address: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // `accept` returns for this connection and finds the flag.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(300));
    }
}

/// The one session at a time, owned by the loop.
pub struct Service {
    storage: PathBuf,
    loopback: bool,
    session: std::sync::Mutex<Option<Session>>,
    sender: Sender<Event>,
    pub receiver: std::sync::mpsc::Receiver<Event>,
}

impl Service {
    pub fn new(storage: PathBuf, loopback: bool) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            storage,
            loopback,
            session: std::sync::Mutex::new(None),
            sender,
            receiver,
        }
    }

    /// Starts listening for `kind` (ending any session before); its address and QR code.
    pub fn start(&self, kind: Kind) -> Result<(String, usize, Vec<bool>), String> {
        let mut slot = self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = None;
        let session = start(
            kind,
            self.storage.clone(),
            self.sender.clone(),
            self.loopback,
        )?;
        let shown = (session.url.clone(), session.size, session.modules.clone());
        *slot = Some(session);
        Ok(shown)
    }

    pub fn stop(&self) {
        *self
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

/// Starts a session for `kind`. `storage`: the folder beside the preferences. `loopback`: on
/// the desktop preview, listen on 127.0.0.1 only.
pub fn start(
    kind: Kind,
    storage: PathBuf,
    events: Sender<Event>,
    loopback: bool,
) -> Result<Session, String> {
    let ip = if loopback {
        Ipv4Addr::LOCALHOST
    } else {
        local_address().ok_or("noNetwork")?
    };
    let listener = TcpListener::bind((ip, 0)).map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let token = token()?;
    let url = format!("http://{address}/{token}/");
    let code = qrcodegen::QrCode::encode_text(&url, qrcodegen::QrCodeEcc::Medium)
        .map_err(|error| format!("{error:?}"))?;
    let size = code.size();
    let modules = (0..size)
        .flat_map(|y| (0..size).map(move |x| (x, y)))
        .map(|(x, y)| code.get_module(x, y))
        .collect();
    let stop = Arc::new(AtomicBool::new(false));
    let running = stop.clone();
    let host = address.to_string();
    std::thread::Builder::new()
        .name("lounge-companion".into())
        .spawn(move || {
            crate::background_priority();
            for stream in listener.incoming() {
                if running.load(Ordering::Acquire) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                if !stream.peer_addr().is_ok_and(|peer| local_peer(peer.ip())) {
                    continue;
                }
                let _ = stream.set_read_timeout(Some(STALL));
                let _ = stream.set_write_timeout(Some(STALL));
                serve(&mut stream, kind, &host, &token, &storage, &events);
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(Session {
        url,
        size: size as usize,
        modules,
        address,
        stop,
    })
}

/// The address of the TV on its local network: the interface its default route leaves by.
/// Connecting a UDP socket sends nothing; it only picks the route.
fn local_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() => Some(ip),
        _ => None,
    }
}

/// Loopback, or a private or link-local IPv4 peer: someone on the TV's own network.
fn local_peer(ip: IpAddr) -> bool {
    ip.is_loopback() || matches!(ip, IpAddr::V4(ip) if ip.is_private() || ip.is_link_local())
}

/// Ten base58 characters from the kernel's random source.
fn token() -> Result<String, String> {
    let mut bytes = [0u8; 64];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("random source: {error}"))?;
    // 232 is the largest multiple of 58 under 256: no bias toward the first letters.
    let token: String = bytes
        .iter()
        .filter(|byte| **byte < 232)
        .take(10)
        .map(|byte| TOKEN_ALPHABET[(byte % 58) as usize] as char)
        .collect();
    (token.len() == 10)
        .then_some(token)
        .ok_or_else(|| "random source: too few bytes".into())
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
}

fn read_head(stream: &mut impl Read) -> Result<(Request, Vec<u8>), String> {
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        if bytes.len() > 16 * 1024 {
            return Err("headers too large".into());
        }
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("closed".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let head = std::str::from_utf8(&bytes[..end]).map_err(|_| "bad request")?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(path), Some("HTTP/1.1"), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        return Err("bad request".into());
    };
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("bad header")?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    let rest = bytes[end + 4..].to_vec();
    Ok((
        Request {
            method: method.into(),
            path: path.into(),
            headers,
        },
        rest,
    ))
}

fn serve(
    stream: &mut TcpStream,
    kind: Kind,
    host: &str,
    token: &str,
    storage: &std::path::Path,
    events: &Sender<Event>,
) {
    let Ok((request, mut body)) = read_head(stream) else {
        return;
    };
    let page = format!("/{token}/");
    // Another site in the phone's browser can't post here: the request must name this server
    // and, when the browser says where it came from, come from its own page.
    let foreign = request.headers.get("host").map(String::as_str) != Some(host)
        || request
            .headers
            .get("origin")
            .is_some_and(|origin| *origin != format!("http://{host}"))
        || request
            .headers
            .get("sec-fetch-site")
            .is_some_and(|site| site == "cross-site");
    let send = |event| {
        let _ = crate::wake::sent(events.send(event));
    };
    match (request.method.as_str(), request.path.strip_prefix(&page)) {
        ("GET", Some("")) => {
            send(Event::Opened);
            let _ = respond(
                stream,
                "200 OK",
                "text/html; charset=utf-8",
                &page_html(kind),
            );
        }
        ("GET", Some("app.js")) => {
            let _ = respond(
                stream,
                "200 OK",
                "text/javascript; charset=utf-8",
                PAGE_SCRIPT.as_bytes(),
            );
        }
        ("POST", Some("photo")) if !foreign => {
            let length = request
                .headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok());
            let Some(length) = length.filter(|length| *length > 0 && *length <= MAX_BODY) else {
                let _ = respond_json(stream, "413 Content Too Large", false, "tooLarge");
                return;
            };
            send(Event::Receiving);
            let mut chunk = vec![0u8; 64 * 1024];
            while body.len() < length {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => {
                        send(Event::Failed("uploadStopped".into()));
                        return;
                    }
                    Ok(read) => body.extend_from_slice(&chunk[..read]),
                }
            }
            body.truncate(length);
            let saved = match kind {
                Kind::Wallpaper => crate::photo::save_wallpaper(&body, storage),
                Kind::Avatar => crate::photo::save_avatar(&body, storage),
            };
            match saved {
                Ok(name) => {
                    let _ = respond_json(stream, "200 OK", true, "");
                    send(Event::Saved(kind, name));
                }
                Err(error) => {
                    eprintln!("Lounge photo: {error}");
                    let _ = respond_json(stream, "422 Unprocessable Content", false, "notAPhoto");
                    send(Event::Failed("notAPhoto".into()));
                }
            }
        }
        ("GET", None) if request.path == format!("/{token}") => {
            let _ = write!(
                stream,
                "HTTP/1.1 303 See Other\r\nLocation: {page}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        }
        _ => {
            let _ = respond(stream, "404 Not Found", "text/plain", b"Not found");
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; \
         img-src blob:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn respond_json(
    stream: &mut TcpStream,
    status: &str,
    ok: bool,
    error: &str,
) -> std::io::Result<()> {
    let body = serde_json::json!({ "ok": ok, "error": error }).to_string();
    respond(stream, status, "application/json", body.as_bytes())
}

/// The phone's page: one button, a preview, Send. The words are filled in by `kind`.
fn page_html(kind: Kind) -> Vec<u8> {
    let (title, hint, width, height) = match kind {
        Kind::Wallpaper => (
            crate::i18n::t("phoneWallpaperTitle"),
            crate::i18n::t("phoneWallpaperHint"),
            crate::photo::WALLPAPER_SIZE.0,
            crate::photo::WALLPAPER_SIZE.1,
        ),
        Kind::Avatar => (
            crate::i18n::t("phoneAvatarTitle"),
            crate::i18n::t("phoneAvatarHint"),
            crate::photo::AVATAR_SIDE,
            crate::photo::AVATAR_SIDE,
        ),
    };
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="color-scheme" content="dark">
<title>Lounge</title>
<style>
*{{box-sizing:border-box;-webkit-tap-highlight-color:transparent}}
html{{background:#07080f}}
body{{margin:0;min-height:100svh;color:#f5f6fa;font:17px/1.4 -apple-system,BlinkMacSystemFont,"SF Pro Text",system-ui,sans-serif;
-webkit-font-smoothing:antialiased;overflow-x:hidden}}
/* The page sits on the photo itself, blurred, as iOS sheets sit on what is behind them; before a
   photo, on soft light. */
.backdrop{{position:fixed;inset:-12%;z-index:-1;background:
radial-gradient(60% 50% at 20% 15%,#4a3f9a 0,transparent 70%),
radial-gradient(55% 45% at 85% 30%,#1f6f8b 0,transparent 70%),
radial-gradient(70% 60% at 50% 100%,#6b2f5e 0,transparent 70%),#0b0c18;
background-size:cover;background-position:center;filter:blur(48px) saturate(1.5);transform:translateZ(0);
transition:opacity .5s ease}}
.backdrop::after{{content:"";position:absolute;inset:0;background:rgba(6,7,14,.45)}}
main{{max-width:520px;margin:0 auto;padding:max(24px,env(safe-area-inset-top)) 20px max(28px,env(safe-area-inset-bottom));
display:flex;flex-direction:column;gap:18px}}
h1{{margin:6px 4px 0;font:700 34px/1.15 -apple-system,BlinkMacSystemFont,"SF Pro Display",system-ui,sans-serif;letter-spacing:-.6px}}
.hint{{margin:0 4px;color:rgba(235,236,245,.72);font-size:16px}}
/* Liquid Glass: a clear pane that bends what is behind it, with a bright rim on its upper edge. */
.glass{{background:rgba(255,255,255,.08);-webkit-backdrop-filter:blur(28px) saturate(1.8);backdrop-filter:blur(28px) saturate(1.8);
border:1px solid rgba(255,255,255,.16);box-shadow:inset 0 1px 0 rgba(255,255,255,.35),inset 0 -1px 0 rgba(255,255,255,.06),0 18px 50px rgba(0,0,0,.35)}}
.card{{border-radius:30px;padding:14px;display:flex;flex-direction:column;gap:14px}}
.frame{{width:100%;aspect-ratio:{width}/{height};border-radius:{radius};overflow:hidden;background:rgba(255,255,255,.05);
touch-action:none;cursor:grab;box-shadow:inset 0 0 0 1px rgba(255,255,255,.1)}}
.frame:active{{cursor:grabbing}}
.frame canvas{{width:100%;height:100%;display:block}}
.round{{width:min(72%,280px);margin:6px auto 0}}
.empty{{display:grid;place-items:center;color:rgba(235,236,245,.5);font-size:15px}}
.tools{{display:flex;flex-direction:column;gap:10px;padding:0 6px 4px}}
.tools[hidden]{{display:none}}
#tip{{margin:0;text-align:center;font-size:14px;color:rgba(235,236,245,.62)}}
input[type=range]{{-webkit-appearance:none;appearance:none;width:100%;height:28px;background:transparent;margin:0}}
input[type=range]::-webkit-slider-runnable-track{{height:6px;border-radius:3px;background:rgba(255,255,255,.22)}}
input[type=range]::-moz-range-track{{height:6px;border-radius:3px;background:rgba(255,255,255,.22)}}
input[type=range]::-webkit-slider-thumb{{-webkit-appearance:none;width:28px;height:28px;margin-top:-11px;border-radius:50%;
background:#fff;box-shadow:0 2px 10px rgba(0,0,0,.35),inset 0 0 0 .5px rgba(0,0,0,.08)}}
input[type=range]::-moz-range-thumb{{width:28px;height:28px;border:0;border-radius:50%;background:#fff;box-shadow:0 2px 10px rgba(0,0,0,.35)}}
.actions{{display:flex;flex-direction:column;gap:12px}}
.button{{appearance:none;border:0;border-radius:999px;min-height:56px;padding:16px 22px;font-family:inherit;font-size:17px;font-weight:600;line-height:1.2;text-align:center;width:100%;display:block;cursor:pointer;color:#f5f6fa;
transition:transform .18s cubic-bezier(.2,.8,.2,1),opacity .2s ease,background-color .2s ease}}
.button:active{{transform:scale(.97)}}
.primary{{background:rgba(255,255,255,.92);color:#0b0c18;border:1px solid rgba(255,255,255,.6);
box-shadow:inset 0 1px 0 #fff,0 10px 30px rgba(0,0,0,.3)}}
.button:disabled{{opacity:.38;transform:none}}
#status{{min-height:1.4em;margin:2px 0 0;text-align:center;color:rgba(235,236,245,.8);transition:opacity .25s ease}}
@media (prefers-reduced-motion:reduce){{*{{transition:none!important}}}}
@media (prefers-reduced-transparency:reduce){{.glass{{background:rgba(40,42,58,.96);-webkit-backdrop-filter:none;backdrop-filter:none}}}}
</style></head><body>
<div class="backdrop" id="backdrop"></div>
<main>
<h1>{title}</h1>
<p class="hint">{hint}</p>
<section class="glass card">
<div class="frame{round}" id="frame"><canvas id="canvas"></canvas></div>
<div class="tools" id="tools" hidden>
<input id="zoom" type="range" min="1" max="4" step="0.01" value="1" aria-label="{zoom}">
<p id="tip">{tip}</p>
</div>
</section>
<div class="actions">
<label class="button glass" for="file">{choose}</label>
<input id="file" type="file" accept="image/*" style="display:none">
<button class="button primary" id="send" disabled>{send}</button>
</div>
<p id="status" role="status" data-sending="{sending}" data-done="{done}" data-failed="{failed}" data-closed="{closed}" data-width="{width}" data-height="{height}"></p>
</main>
<script src="app.js"></script>
</body></html>"#,
        title = escape(title),
        hint = escape(hint),
        choose = escape(crate::i18n::t("phoneChoose")),
        send = escape(crate::i18n::t("phoneSend")),
        sending = escape(crate::i18n::t("phoneSending")),
        done = escape(crate::i18n::t("phoneDone")),
        failed = escape(crate::i18n::t("phoneFailed")),
        closed = escape(crate::i18n::t("phoneClosed")),
        tip = escape(crate::i18n::t("phoneTip")),
        zoom = escape(crate::i18n::t("phoneZoom")),
        radius = if kind == Kind::Avatar { "50%" } else { "14px" },
        round = if kind == Kind::Avatar { " round" } else { "" },
    )
    .into_bytes()
}

/// Fits the chosen photo on the phone (cover, centred) and sends it as a JPEG.
const PAGE_SCRIPT: &str = r#"const $ = (id) => document.getElementById(id);
const status = $('status'), canvas = $('canvas'), zoom = $('zoom'), send = $('send'), tools = $('tools');
const width = +status.dataset.width, height = +status.dataset.height;
const context = canvas.getContext('2d');
// The photo, and where it sits: `scale` × the size that just covers the frame, centred at
// (`x`, `y`) in the photo's own pixels. What the frame shows is exactly what the TV gets.
let photo = null, cover = 1, x = 0, y = 0;
const size = () => {
  const box = canvas.getBoundingClientRect();
  canvas.width = Math.round(box.width * devicePixelRatio);
  canvas.height = Math.round(box.height * devicePixelRatio);
};
// Kept inside the photo: the frame never shows past its edge.
const clamp = () => {
  const s = cover * +zoom.value, halfW = width / s / 2, halfH = height / s / 2;
  x = Math.min(Math.max(x, halfW), photo.width - halfW);
  y = Math.min(Math.max(y, halfH), photo.height - halfH);
};
const draw = (target, w, h) => {
  const s = cover * +zoom.value * (w / width);
  const ctx = target.getContext('2d');
  ctx.imageSmoothingQuality = 'high';
  ctx.clearRect(0, 0, w, h);
  ctx.drawImage(photo, w / 2 - x * s, h / 2 - y * s, photo.width * s, photo.height * s);
};
const show = () => { if (photo) { clamp(); draw(canvas, canvas.width, canvas.height); } };
addEventListener('resize', () => { size(); show(); });
size();
$('file').addEventListener('change', async (event) => {
  const file = event.target.files[0];
  if (!file) return;
  status.textContent = '';
  try {
    // Older Safari can't make a bitmap of every photo it can show: then through an image.
    photo = await createImageBitmap(file).catch(() => new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = reject;
      image.src = URL.createObjectURL(file);
    }));
    cover = Math.max(width / photo.width, height / photo.height);
    x = photo.width / 2; y = photo.height / 2; zoom.value = 1;
    tools.hidden = false;
    // The page takes on the photo, blurred behind the glass.
    $('backdrop').style.background = `center / cover no-repeat url(${URL.createObjectURL(file)})`;
    size(); show();
    send.disabled = false;
  } catch {
    status.textContent = status.dataset.failed;
  }
});
zoom.addEventListener('input', show);
// Drag to move the photo in the frame.
let drag = null;
canvas.addEventListener('pointerdown', (event) => {
  if (!photo) return;
  drag = { id: event.pointerId, px: event.clientX, py: event.clientY, x, y };
  canvas.setPointerCapture(event.pointerId);
});
canvas.addEventListener('pointermove', (event) => {
  if (!drag || event.pointerId !== drag.id) return;
  const s = cover * +zoom.value * (canvas.getBoundingClientRect().width / width);
  x = drag.x - (event.clientX - drag.px) / s;
  y = drag.y - (event.clientY - drag.py) / s;
  show();
});
const release = () => { drag = null; };
canvas.addEventListener('pointerup', release);
canvas.addEventListener('pointercancel', release);
send.addEventListener('click', async () => {
  if (!photo) return;
  send.disabled = true;
  status.textContent = status.dataset.sending;
  const out = document.createElement('canvas');
  out.width = width; out.height = height;
  draw(out, width, height);
  const blob = await new Promise((resolve) => out.toBlob(resolve, 'image/jpeg', 0.92));
  try {
    const response = await fetch('photo', { method: 'POST', headers: { 'Content-Type': 'image/jpeg' }, body: blob });
    const reply = await response.json();
    status.textContent = reply.ok ? status.dataset.done : status.dataset.failed;
  } catch {
    // No answer at all: the TV stopped listening (its code was closed).
    status.textContent = status.dataset.closed;
  }
  send.disabled = false;
});
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_ten_unambiguous_characters_and_differ() {
        let (a, b) = (token().unwrap(), token().unwrap());
        assert_eq!(a.len(), 10);
        assert!(a.bytes().all(|byte| TOKEN_ALPHABET.contains(&byte)));
        assert_ne!(a, b);
    }

    #[test]
    fn only_the_local_network_is_served() {
        assert!(local_peer("192.168.1.20".parse().unwrap()));
        assert!(local_peer("10.0.0.5".parse().unwrap()));
        assert!(local_peer("127.0.0.1".parse().unwrap()));
        assert!(!local_peer("8.8.8.8".parse().unwrap()));
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    fn exchange(address: &str, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        // A refused upload is answered before its body is read, and the rest is reset.
        let _ = stream.write_all(request);
        let mut reply = Vec::new();
        let _ = stream.read_to_end(&mut reply);
        String::from_utf8_lossy(&reply).into_owned()
    }

    #[test]
    fn a_photo_sent_from_the_page_becomes_the_wallpaper_and_strangers_are_refused() {
        let storage = std::env::temp_dir().join(format!("lounge-companion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&storage);
        let service = Service::new(storage.clone(), true);
        let (url, size, modules) = service.start(Kind::Wallpaper).unwrap();
        assert_eq!(modules.len(), size * size);
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_once('/').unwrap();
        let page = format!("/{path}");
        let get = |target: &str| {
            exchange(
                host,
                format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes(),
            )
        };
        assert!(get(&page).starts_with("HTTP/1.1 200"));
        assert!(matches!(service.receiver.recv().unwrap(), Event::Opened));
        assert!(get(&format!("{page}app.js")).contains("createImageBitmap"));
        // A wrong token finds nothing.
        assert!(get("/wrongtoken1/").starts_with("HTTP/1.1 404"));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&image::RgbImage::from_pixel(
                1920,
                1080,
                image::Rgb([40, 90, 160]),
            ))
            .unwrap();
        let post = |origin: &str| {
            let mut request = format!(
                "POST {page}photo HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                jpeg.len()
            )
            .into_bytes();
            request.extend_from_slice(&jpeg);
            exchange(host, &request)
        };
        // Another site in the phone's browser can't send one.
        assert!(post("http://example.com").starts_with("HTTP/1.1 404"));
        assert!(post(&format!("http://{host}")).starts_with("HTTP/1.1 200"));
        assert!(matches!(service.receiver.recv().unwrap(), Event::Receiving));
        let Event::Saved(Kind::Wallpaper, id) = service.receiver.recv().unwrap() else {
            panic!("not saved");
        };
        assert_eq!(crate::photo::saved_wallpaper(&storage), Some(id));
        // Stopped, nothing answers.
        service.stop();
        std::thread::sleep(Duration::from_millis(100));
        assert!(TcpStream::connect(host).is_err());
        let _ = std::fs::remove_dir_all(&storage);
    }
}
