//! The design references page: a small HTTP server on 127.0.0.1 where the
//! user sees the designers' references at full size, approves or rejects
//! each one, and uploads images of their own. Every account on the machine
//! can reach 127.0.0.1, so every path starts with a random key only the
//! user's board shows; without it, everything is 404. It runs as a thread of
//! the dispatcher.

use crate::references::{self, Answer, FileKind, ReferenceSet, SetStatus, Verdict};
use crate::task::Task;
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::Duration;

/// The most a request may carry: an upload of the largest image, and its form.
const MAX_BODY: usize = 11 * 1024 * 1024;
const MAX_HEAD: usize = 16 * 1024;

const PAGE_POLICY: &str = "default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'; script-src 'self'; frame-src 'self'; form-action 'self'; base-uri 'none'";

/// The page's only script, its own file (no inline script runs): drop images
/// on the box, or paste a screenshot anywhere, and they fill the form.
const PAGE_SCRIPT: &str = r#"const zone = document.getElementById("drop");
const input = document.getElementById("image");
const chosen = document.getElementById("chosen");
function show(files) {
  chosen.textContent = files.length ? [...files].map((file) => file.name || "pasted image").join(", ") : "";
}
function take(files) {
  const list = new DataTransfer();
  for (const file of files) if (file.type.startsWith("image/")) list.items.add(file);
  if (!list.files.length) return;
  input.files = list.files;
  show(list.files);
}
zone.addEventListener("dragover", (event) => { event.preventDefault(); zone.classList.add("over"); });
zone.addEventListener("dragleave", () => zone.classList.remove("over"));
zone.addEventListener("drop", (event) => { event.preventDefault(); zone.classList.remove("over"); take(event.dataTransfer.files); });
zone.addEventListener("click", (event) => { if (event.target === zone) input.click(); });
document.addEventListener("paste", (event) => { if (event.clipboardData.files.length) { event.preventDefault(); take(event.clipboardData.files); } });
input.addEventListener("change", () => show(input.files));
"#;
/// A mockup or SVG: no scripts, no network, its own sandbox even when opened
/// on its own.
const MOCKUP_POLICY: &str = "default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline' data:; font-src data:; sandbox";

fn key_path(task: &Task) -> PathBuf {
    task.dir().join("references").join("key")
}

fn error_path(task: &Task) -> PathBuf {
    task.dir().join("references").join("page-error")
}

/// The page's key, made on first use: 32 random bytes, in hex.
pub fn key(task: &Task) -> Result<String> {
    let path = key_path(task);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim().to_string();
        if text.len() == 64 {
            return Ok(text);
        }
    }
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .context("reading random bytes for the references page's key")?;
    let key: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(key.as_bytes())?;
    Ok(key)
}

/// Where the user opens the page.
pub fn url(task: &Task) -> Result<String> {
    let port = task.config()?.references_port;
    Ok(format!("http://127.0.0.1:{port}/r/{}/", key(task)?))
}

/// Why the page is not being served, when the dispatcher could not open it.
pub fn unavailable(task: &Task) -> Option<String> {
    std::fs::read_to_string(error_path(task))
        .ok()
        .map(|text| text.trim().to_string())
}

/// Opens the page for as long as this process runs. A port in use is
/// written down for the board, and the CLI still answers.
pub fn serve_in_background(task: &Task) -> Result<()> {
    let port = task.config()?.references_port;
    let key = key(task)?;
    match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => {
            let _ = std::fs::remove_file(error_path(task));
            let task = task.clone();
            std::thread::spawn(move || serve(&task, &key, &listener));
            Ok(())
        }
        Err(error) => {
            let reason = format!("port {port} in use ({error})");
            std::fs::write(error_path(task), &reason)?;
            bail!("the references page could not open: {reason}")
        }
    }
}

fn serve(task: &Task, key: &str, listener: &TcpListener) {
    for stream in listener.incoming().flatten() {
        let (task, key) = (task.clone(), key.to_string());
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
            let _ = handle_connection(&task, &key, stream);
        });
    }
}

#[derive(Debug, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub policy: &'static str,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

impl Response {
    fn page(status: u16, body: String) -> Self {
        Response {
            status,
            content_type: "text/html; charset=utf-8",
            policy: PAGE_POLICY,
            location: None,
            body: body.into_bytes(),
        }
    }

    fn not_found() -> Self {
        Response::page(404, "not found".to_string())
    }

    fn back_to(location: String) -> Self {
        Response {
            status: 303,
            content_type: "text/plain",
            policy: PAGE_POLICY,
            location: Some(location),
            body: Vec::new(),
        }
    }
}

fn read_request(stream: &mut impl BufRead) -> Result<Request> {
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = stream.take(MAX_HEAD as u64).read_until(b'\n', &mut line)?;
        ensure!(read > 0, "the connection closed");
        head.extend_from_slice(&line);
        ensure!(head.len() <= MAX_HEAD, "the request's head is too long");
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let head = String::from_utf8(head).context("the request's head is not text")?;
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    let (mut length, mut content_type) = (0usize, String::new());
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.trim().parse().context("a wrong Content-Length")?,
            "content-type" => content_type = value.trim().to_string(),
            _ => {}
        }
    }
    ensure!(length <= MAX_BODY, "the request is too large");
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        content_type,
        body,
    })
}

fn handle_connection(task: &Task, key: &str, stream: TcpStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let response = match read_request(&mut reader) {
        Ok(request) => respond(task, key, &request),
        Err(error) => Response::page(400, escape(&format!("{error:#}"))),
    };
    write_response(stream, &response)
}

fn write_response(mut stream: impl Write, response: &Response) -> Result<()> {
    let reason = match response.status {
        200 => "OK",
        303 => "See Other",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Security-Policy: {}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nX-Frame-Options: SAMEORIGIN\r\nConnection: close\r\n",
        response.status,
        response.content_type,
        response.body.len(),
        response.policy
    );
    if let Some(location) = &response.location {
        head.push_str(&format!("Location: {location}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

/// The answer to one request. Every path is under `/r/<key>/`.
pub fn respond(task: &Task, key: &str, request: &Request) -> Response {
    let prefix = format!("/r/{key}/");
    let Some(rest) = request.path.strip_prefix(&prefix) else {
        return Response::not_found();
    };
    let rest = rest.split('?').next().unwrap_or("");
    let parts: Vec<&str> = rest.split('/').collect();
    let result = match (request.method.as_str(), parts.as_slice()) {
        ("GET", [""]) => index(task, None).map(|page| Response::page(200, page)),
        ("GET", ["file", step, round, name]) => Ok(round_file(task, step, round, name)),
        ("GET", ["user", name]) => Ok(user_file(task, name)),
        ("POST", ["answer", step]) => {
            answer(task, step, &request.body).map(|()| Response::back_to(prefix.clone()))
        }
        ("GET", ["page.js"]) => Ok(Response {
            status: 200,
            content_type: "text/javascript; charset=utf-8",
            policy: PAGE_POLICY,
            location: None,
            body: PAGE_SCRIPT.as_bytes().to_vec(),
        }),
        ("POST", ["upload"]) => upload(task, request).map(|()| Response::back_to(prefix.clone())),
        ("POST", ["remove", id]) => {
            references::remove_given(task, id).map(|()| Response::back_to(prefix.clone()))
        }
        _ => return Response::not_found(),
    };
    result.unwrap_or_else(|error| {
        let page = index(task, Some(&format!("{error:#}")))
            .unwrap_or_else(|_| escape(&format!("{error:#}")));
        Response::page(400, page)
    })
}

fn file_response(path: PathBuf, kind: FileKind) -> Response {
    match std::fs::read(path) {
        Ok(body) => Response {
            status: 200,
            content_type: kind.content_type(),
            policy: MOCKUP_POLICY,
            location: None,
            body,
        },
        Err(_) => Response::not_found(),
    }
}

/// A file of a round, only by a name the state lists for it.
fn round_file(task: &Task, step: &str, round: &str, name: &str) -> Response {
    let Ok(state) = task.state() else {
        return Response::not_found();
    };
    let found = state.references.iter().find_map(|set| {
        let item = set.items.iter().find(|item| item.file == name)?;
        (set.step == step && set.round.to_string() == round).then(|| (set.folder(task), item.kind))
    });
    match found {
        Some((folder, kind)) => file_response(folder.join(name), kind),
        None => Response::not_found(),
    }
}

fn user_file(task: &Task, name: &str) -> Response {
    let Ok(state) = task.state() else {
        return Response::not_found();
    };
    match state.user_references.iter().find(|item| item.file == name) {
        Some(item) => file_response(references::user_folder(task).join(name), item.kind),
        None => Response::not_found(),
    }
}

/// `v_r1=approved&c_r1=…&comment=…` into each reference's answer.
fn answer(task: &Task, step: &str, body: &[u8]) -> Result<()> {
    let form = form_fields(body);
    let mut answers = BTreeMap::new();
    for (name, value) in &form {
        let Some(id) = name.strip_prefix("v_") else {
            continue;
        };
        let verdict = match value.as_str() {
            "approved" => Verdict::Approved,
            "rejected" => Verdict::Rejected,
            other => bail!("{other} is not approved or rejected"),
        };
        answers.insert(
            id.to_string(),
            Answer {
                verdict,
                comment: form.get(&format!("c_{id}")).cloned(),
            },
        );
    }
    references::answer(
        task,
        step,
        &answers,
        form.get("comment").map(String::as_str),
    )?;
    Ok(())
}

fn upload(task: &Task, request: &Request) -> Result<()> {
    let boundary = request
        .content_type
        .split(';')
        .find_map(|part| part.trim().strip_prefix("boundary="))
        .map(|boundary| boundary.trim_matches('"').to_string())
        .context("the upload is not a multipart form")?;
    let parts = multipart(&request.body, &boundary);
    let images: Vec<&Part> = parts
        .iter()
        .filter(|part| part.name == "image" && !part.body.is_empty())
        .collect();
    ensure!(!images.is_empty(), "choose, drop or paste an image first");
    let note = parts
        .iter()
        .find(|part| part.name == "note")
        .map(|part| String::from_utf8_lossy(&part.body).to_string())
        .unwrap_or_default();
    // Every one is checked before any is kept: a wrong file keeps none.
    let mut kinds = Vec::new();
    for image in &images {
        kinds.push(
            FileKind::of(&image.body)
                .filter(|kind| kind.is_image())
                .context("one of them is not a PNG, JPEG, WebP or SVG image")?,
        );
    }
    for (image, kind) in images.iter().zip(kinds) {
        references::give_bytes(task, &image.body, kind, &note)?;
    }
    Ok(())
}

#[derive(Debug)]
struct Part {
    name: String,
    body: Vec<u8>,
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

/// The parts of a `multipart/form-data` body.
fn multipart(body: &[u8], boundary: &str) -> Vec<Part> {
    let delimiter = format!("--{boundary}").into_bytes();
    let mut parts = Vec::new();
    let Some(mut start) = find(body, &delimiter, 0) else {
        return parts;
    };
    loop {
        start += delimiter.len();
        if body.get(start..start + 2) == Some(b"--") {
            break;
        }
        let Some(head_end) = find(body, b"\r\n\r\n", start) else {
            break;
        };
        let Some(next) = find(body, &delimiter, head_end) else {
            break;
        };
        let head = String::from_utf8_lossy(&body[start..head_end]);
        let name = head
            .split(';')
            .find_map(|field| field.trim().strip_prefix("name="))
            .map(|name| {
                name.split(['\r', '\n'])
                    .next()
                    .unwrap_or("")
                    .trim_matches('"')
                    .to_string()
            })
            .unwrap_or_default();
        let end = next.saturating_sub(2).max(head_end + 4);
        parts.push(Part {
            name,
            body: body[head_end + 4..end].to_vec(),
        });
        start = next;
    }
    parts
}

fn form_fields(body: &[u8]) -> BTreeMap<String, String> {
    String::from_utf8_lossy(body)
        .split('&')
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            Some((decode(name), decode(value)))
        })
        .collect()
}

/// `application/x-www-form-urlencoded` text.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&text[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const STYLE: &str = "body{font:15px/1.5 system-ui,sans-serif;margin:0 auto;max-width:1200px;padding:24px;color:#1d1d1f;background:#f6f6f4}h1{font-size:22px}h2{font-size:18px;margin-top:32px}.card{background:#fff;border:1px solid #ddd;border-radius:8px;padding:16px;margin:16px 0}.card img{max-width:100%;border:1px solid #eee}.card iframe{width:100%;height:720px;border:1px solid #eee;background:#fff}.note{color:#444;margin:8px 0}.choice label{margin-right:16px}textarea{width:100%;min-height:48px;font:inherit}button{font:inherit;padding:8px 20px;border-radius:6px;border:1px solid #333;background:#1d1d1f;color:#fff;cursor:pointer}.error{background:#fde8e8;border:1px solid #e0a0a0;padding:12px;border-radius:6px}.muted{color:#777}.given{display:flex;flex-wrap:wrap;gap:12px}.given figure{margin:0;width:220px}.given img{width:220px;border:1px solid #ddd}.drop{border:2px dashed #bbb;border-radius:8px;padding:28px;text-align:center;color:#555;cursor:pointer}.drop.over{border-color:#1d1d1f;background:#eee}.pick{text-decoration:underline;cursor:pointer}.quiet{background:none;color:#a33;border:none;padding:2px 0}";

fn reference_card(set: &ReferenceSet, item: &references::Reference) -> String {
    let source = format!(
        "file/{}/{}/{}",
        escape(&set.step),
        set.round,
        escape(&item.file)
    );
    let shown = if item.kind.is_image() {
        format!(
            "<a href=\"{source}\" target=\"_blank\" rel=\"noreferrer\"><img src=\"{source}\" alt=\"{}\"></a>",
            escape(&item.id)
        )
    } else {
        format!(
            "<iframe sandbox src=\"{source}\" title=\"{}\"></iframe><div class=\"muted\"><a href=\"{source}\" target=\"_blank\" rel=\"noreferrer\">open full page</a></div>",
            escape(&item.id)
        )
    };
    format!(
        "<div class=\"card\"><p class=\"note\"><strong>{id}</strong> · {note}</p>{shown}<div class=\"choice\"><label><input type=\"radio\" name=\"v_{id}\" value=\"approved\" required> Approve</label><label><input type=\"radio\" name=\"v_{id}\" value=\"rejected\"> Reject</label></div><textarea name=\"c_{id}\" placeholder=\"Comment (optional): what to keep, what to change\"></textarea></div>",
        id = escape(&item.id),
        note = escape(&item.note),
    )
}

fn waiting_set(set: &ReferenceSet) -> String {
    let cards: String = set
        .items
        .iter()
        .map(|item| reference_card(set, item))
        .collect();
    format!(
        "<h2>{step} · {agent} · round {round}</h2><form method=\"post\" action=\"answer/{step}\">{cards}<p>A comment on the whole round (optional):</p><textarea name=\"comment\"></textarea><p><button type=\"submit\">Send my answer</button> <span class=\"muted\">Approve at least one and the designer plans from it; reject all and it shows a new round.</span></p></form>",
        step = escape(&set.step),
        agent = escape(&set.agent),
        round = set.round,
    )
}

fn answered_line(set: &ReferenceSet) -> String {
    let word = match set.status {
        SetStatus::Approved => "approved",
        SetStatus::Rejected => "rejected",
        SetStatus::Drafting => "being drafted",
        SetStatus::Waiting => "waiting",
    };
    let approved: Vec<&str> = set
        .items
        .iter()
        .filter(|item| item.verdict == Some(Verdict::Approved))
        .map(|item| item.id.as_str())
        .collect();
    let which = if approved.is_empty() {
        String::new()
    } else {
        format!(": {}", approved.join(", "))
    };
    format!(
        "<li>{} round {} ({}) — {word}{}</li>",
        escape(&set.step),
        set.round,
        escape(&set.agent),
        escape(&which)
    )
}

/// The page: what waits for the user's answer, the images they gave (and
/// where they give more), and what they answered before.
pub fn index(task: &Task, error: Option<&str>) -> Result<String> {
    let state = task.state()?;
    let mut body = String::from("<h1>Design references</h1>");
    if let Some(error) = error {
        body.push_str(&format!("<p class=\"error\">{}</p>", escape(error)));
    }
    // What waits for an answer comes first; the images after.
    for set in references::waiting(&state) {
        body.push_str(&waiting_set(set));
    }
    body.push_str("<h2>Your images</h2><p class=\"muted\">The leader and every designer see these; designers build their directions from them.</p><div class=\"given\">");
    for item in &state.user_references {
        let source = format!("user/{}", escape(&item.file));
        body.push_str(&format!(
            "<figure><a href=\"{source}\" target=\"_blank\" rel=\"noreferrer\"><img src=\"{source}\" alt=\"{id}\"></a><figcaption><strong>{id}</strong> {note}</figcaption><form method=\"post\" action=\"remove/{id}\"><button type=\"submit\" class=\"quiet\">Remove</button></form></figure>",
            id = escape(&item.id),
            note = escape(&item.note)
        ));
    }
    body.push_str("</div><form method=\"post\" action=\"upload\" enctype=\"multipart/form-data\" class=\"card\"><div id=\"drop\" class=\"drop\">Drop images here, paste a screenshot (Ctrl+V), or <label class=\"pick\">choose files<input id=\"image\" type=\"file\" name=\"image\" multiple accept=\"image/png,image/jpeg,image/webp,image/svg+xml\" hidden></label><div id=\"chosen\" class=\"muted\"></div></div><p><input type=\"text\" name=\"note\" placeholder=\"What to take from it: the density, the colours, how the cards sit…\" style=\"width:100%\"></p><button type=\"submit\">Give these images</button></form>");
    let answered: String = state
        .references
        .iter()
        .filter(|set| matches!(set.status, SetStatus::Approved | SetStatus::Rejected))
        .rev()
        .take(20)
        .map(answered_line)
        .collect();
    if !answered.is_empty() {
        body.push_str(&format!("<h2>Answered</h2><ul>{answered}</ul>"));
    }
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Design references</title><style>{STYLE}</style></head><body>{body}<script src=\"page.js\"></script></body></html>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Agent, Order, OrderStatus};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    fn waiting_task() -> (tempfile::TempDir, Task, String) {
        let dir = tempfile::TempDir::new().unwrap();
        let config: crate::task::TaskConfig =
            toml::from_str("id = \"t\"\nbranch = \"feat/x\"\nworkspace = \"dev-a\"\n").unwrap();
        let task = Task::create(&crate::home::Home::at(dir.path().to_path_buf()), &config).unwrap();
        task.transaction(|tx| {
            tx.state.agents.push(Agent {
                id: "designer-1".into(),
                role: "designer".into(),
                agent: "codex".into(),
                repo: "app".into(),
                session: String::new(),
                pane: "%1".into(),
                writes: false,
                group: Some(crate::roles::Group::Senior),
                lane: None,
                seat: 0,
                live: true,
                unavailable: false,
                ready: true,
                lost: false,
                usage: None,
                shows_references: true,
            });
            tx.state.orders.push(Order {
                id: "o1".into(),
                step: "G9".into(),
                agent: "designer-1".into(),
                text: "plan".into(),
                allowed: vec![],
                forbidden: vec![],
                check: None,
                base: String::new(),
                status: OrderStatus::Open,
                delivered: true,
                stalled: false,
                plan: None,
                read: vec![],
                targets: vec![],
            });
            Ok(())
        })
        .unwrap();
        let scratch = task.dir().join("workers/designer-1/scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(scratch.join("a.png"), PNG).unwrap();
        std::fs::write(scratch.join("b.html"), b"<!doctype html><p>mock</p>").unwrap();
        references::add(
            &task,
            "designer-1",
            &scratch.join("a.png"),
            "dense <b>table</b>",
        )
        .unwrap();
        references::add(&task, "designer-1", &scratch.join("b.html"), "cards").unwrap();
        references::submit(&task, "designer-1").unwrap();
        let key = key(&task).unwrap();
        (dir, task, key)
    }

    fn get(path: &str) -> Request {
        Request {
            method: "GET".into(),
            path: path.into(),
            content_type: String::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn nothing_is_served_without_the_key() {
        let (_dir, task, key) = waiting_task();
        assert_eq!(key.len(), 64);
        assert_eq!(key, super::key(&task).unwrap(), "the key is kept");
        let mode = std::fs::metadata(key_path(&task))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        for path in ["/", "/r/", "/r/wrong/", "/r/wrong/file/G9/1/r1.png"] {
            assert_eq!(respond(&task, &key, &get(path)).status, 404, "{path}");
        }
        let page = respond(&task, &key, &get(&format!("/r/{key}/")));
        assert_eq!(page.status, 200);
        let text = String::from_utf8(page.body).unwrap();
        assert!(
            text.contains("dense &lt;b&gt;table&lt;/b&gt;"),
            "notes are escaped"
        );
        assert!(text.contains("<iframe sandbox src=\"file/G9/1/r2.html\""));
        assert!(text.contains("<img src=\"file/G9/1/r1.png\""));
        assert_eq!(
            text.matches("<script").count(),
            1,
            "only the page's own file"
        );
        assert!(text.contains("<script src=\"page.js\"></script>"));
        let script = respond(&task, &key, &get(&format!("/r/{key}/page.js")));
        assert_eq!(script.status, 200);
        assert!(String::from_utf8(script.body).unwrap().contains("paste"));
        assert!(page.policy.contains("script-src 'self'"));
    }

    #[test]
    fn only_the_files_of_a_round_are_served_with_a_sandbox() {
        let (_dir, task, key) = waiting_task();
        let mockup = respond(&task, &key, &get(&format!("/r/{key}/file/G9/1/r2.html")));
        assert_eq!(mockup.status, 200);
        assert_eq!(mockup.content_type, "text/html; charset=utf-8");
        assert!(mockup.policy.contains("sandbox"));
        assert!(mockup.policy.contains("default-src 'none'"));
        for path in [
            "file/G9/1/../../../key",
            "file/G9/2/r1.png",
            "file/G1/1/r1.png",
            "user/u1.png",
        ] {
            assert_eq!(
                respond(&task, &key, &get(&format!("/r/{key}/{path}"))).status,
                404,
                "{path}"
            );
        }
    }

    #[test]
    fn an_answer_from_the_page_reaches_the_designer() {
        let (_dir, task, key) = waiting_task();
        let request = Request {
            method: "POST".into(),
            path: format!("/r/{key}/answer/G9"),
            content_type: "application/x-www-form-urlencoded".into(),
            body: b"v_r1=rejected&c_r1=too+busy&v_r2=approved&c_r2=less%20shadow&comment=".to_vec(),
        };
        let response = respond(&task, &key, &request);
        assert_eq!(response.status, 303);
        assert_eq!(
            response.location.as_deref(),
            Some(format!("/r/{key}/").as_str())
        );
        let state = task.state().unwrap();
        let set = references::approved_for(&state, "G9").unwrap();
        assert_eq!(set.items[1].comment.as_deref(), Some("less shadow"));
        assert!(
            state
                .queue
                .iter()
                .any(|message| message.to == "designer-1" && message.text.contains("too busy"))
        );

        let again = respond(&task, &key, &request);
        assert_eq!(again.status, 400, "a round is answered once");
    }

    #[test]
    fn a_partial_answer_shows_the_page_with_why() {
        let (_dir, task, key) = waiting_task();
        let request = Request {
            method: "POST".into(),
            path: format!("/r/{key}/answer/G9"),
            content_type: "application/x-www-form-urlencoded".into(),
            body: b"v_r1=approved".to_vec(),
        };
        let response = respond(&task, &key, &request);
        assert_eq!(response.status, 400);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("r2 not answered")
        );
    }

    #[test]
    fn the_user_uploads_an_image() {
        let (_dir, task, key) = waiting_task();
        let mut body = Vec::new();
        body.extend_from_slice(b"--XyZ\r\nContent-Disposition: form-data; name=\"image\"; filename=\"mine.png\"\r\nContent-Type: image/png\r\n\r\n");
        body.extend_from_slice(PNG);
        body.extend_from_slice(b"\r\n--XyZ\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nlike Linear\r\n--XyZ--\r\n");
        let request = Request {
            method: "POST".into(),
            path: format!("/r/{key}/upload"),
            content_type: "multipart/form-data; boundary=XyZ".into(),
            body,
        };
        assert_eq!(respond(&task, &key, &request).status, 303);
        let state = task.state().unwrap();
        assert_eq!(state.user_references[0].note, "like Linear");
        let served = respond(&task, &key, &get(&format!("/r/{key}/user/u1.png")));
        assert_eq!((served.status, served.body.as_slice()), (200, PNG));
    }

    #[test]
    fn a_request_is_read_from_the_wire() {
        let wire = b"POST /r/k/answer/G9 HTTP/1.1\r\nHost: x\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello";
        let request = read_request(&mut BufReader::new(&wire[..])).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/r/k/answer/G9");
        assert_eq!(request.body, b"hello");
        let huge = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert!(read_request(&mut BufReader::new(huge.as_bytes())).is_err());
    }

    #[test]
    fn the_page_is_served_and_a_taken_port_is_reported() {
        let (_dir, task, key) = waiting_task();
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut config = task.config().unwrap();
        config.references_port = port;
        task.write_config(&config).unwrap();
        serve_in_background(&task).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(stream, "GET /r/{key}/ HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.contains("Referrer-Policy: no-referrer"));
        let error = serve_in_background(&task).unwrap_err().to_string();
        assert!(error.contains(&format!("port {port} in use")), "{error}");
        assert!(unavailable(&task).unwrap().contains("in use"));
    }

    fn upload_of(key: &str, images: &[&[u8]]) -> Request {
        let mut body = Vec::new();
        for (number, image) in images.iter().enumerate() {
            body.extend_from_slice(format!("--XyZ\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{number}.png\"\r\n\r\n").as_bytes());
            body.extend_from_slice(image);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--XyZ\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\ncalm colours\r\n--XyZ--\r\n");
        Request {
            method: "POST".into(),
            path: format!("/r/{key}/upload"),
            content_type: "multipart/form-data; boundary=XyZ".into(),
            body,
        }
    }

    #[test]
    fn several_images_are_given_at_once_or_none_when_one_is_wrong() {
        let (_dir, task, key) = waiting_task();
        let refused = respond(&task, &key, &upload_of(&key, &[PNG, b"#!/bin/sh"]));
        assert_eq!(refused.status, 400);
        assert!(
            task.state().unwrap().user_references.is_empty(),
            "none kept"
        );

        assert_eq!(
            respond(&task, &key, &upload_of(&key, &[PNG, PNG])).status,
            303
        );
        let state = task.state().unwrap();
        let ids: Vec<&str> = state
            .user_references
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["u1", "u2"]);
        assert!(
            state
                .user_references
                .iter()
                .all(|item| item.note == "calm colours")
        );
    }

    #[test]
    fn an_image_given_by_mistake_is_removed_from_the_page() {
        let (_dir, task, key) = waiting_task();
        respond(&task, &key, &upload_of(&key, &[PNG]));
        let remove = Request {
            method: "POST".into(),
            path: format!("/r/{key}/remove/u1"),
            content_type: String::new(),
            body: Vec::new(),
        };
        assert_eq!(respond(&task, &key, &remove).status, 303);
        assert!(task.state().unwrap().user_references.is_empty());
        assert_eq!(
            respond(&task, &key, &get(&format!("/r/{key}/user/u1.png"))).status,
            404
        );
        assert_eq!(respond(&task, &key, &remove).status, 400, "it is gone");
    }
}
