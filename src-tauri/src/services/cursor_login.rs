//! Cursor's separate sign-in for this app (「单独登录」: maintainer approval
//! 2026-11-25, plan 《交接笔记/Cursor-加密凭据-方案.md》; CLI side
//! vibe-cafe/vibe-usage#210).
//!
//! Cursor 3.27+ encrypts the login it keeps, and it is not decrypted. Instead
//! the user signs in once more, for Vibe Usage, through the browser sign-in
//! Cursor's own CLI uses. The CLI does every network step:
//! - `login cursor --json` prints the browser URL, then the sign-in;
//! - `login cursor --refresh --json --cursor-login-stdin` renews it;
//! - every `sync` and `quota fetch` gets it on stdin (`--cursor-login-stdin`),
//!   never in an argument or the environment.
//!
//! The app only keeps it, in its own entry of the current user's Credential
//! Manager (`ai.vibecafe.vibe-usage.windows/cursor/login`), and never writes
//! what the sign-in and renewal commands print to a log, the diagnostics or
//! the sync history. Nothing Cursor stored is read.

use crate::services::credential_store::{self, StoreError};
use crate::services::sync_engine;
use crate::state::AppCtx;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TARGET: &str = "ai.vibecafe.vibe-usage.windows/cursor/login";
/// One entry holds 2560 bytes. A sign-in is two JWTs plus a few fields,
/// usually one entry; a longer one is split across up to `MAX_PARTS`.
const PART_BYTES: usize = 2000;
const MAX_PARTS: usize = 4;
/// Renew this long before the sign-in's `expiresAt` (about 60 days after
/// sign-in), so a computer that stays off for a few days does not lose it.
const REFRESH_LEAD_SECS: i64 = 7 * 24 * 60 * 60;
/// After a renewal that failed for a temporary reason, wait this long.
const REFRESH_HOLD: Duration = Duration::from_secs(6 * 60 * 60);
/// A renewed sign-in rejected within this long, before Cursor accepted it
/// once, has ended. Later rejections get a renewal of their own.
const FRESH_WINDOW: Duration = Duration::from_secs(60 * 60);
/// The CLI waits up to 5 minutes for the browser; the launcher may first need
/// the npm registry.
const LOGIN_DEADLINE: Duration = Duration::from_secs(6 * 60);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(60);
const SUPPORT_TIMEOUT: Duration = Duration::from_secs(30);
/// While waiting for the browser, how often the sign-in's routes are read
/// again (`route_changed`).
const ROUTE_CHECK: Duration = Duration::from_secs(3);
/// The hosts the sign-in's own requests reach: `api2.cursor.sh` waits for the
/// confirmation, `cursor.com` reads the account email.
const SIGN_IN_HOSTS: [&str; 2] = ["api2.cursor.sh", "cursor.com"];
/// A CLI without `login cursor` is asked again after this long.
const UNSUPPORTED_RECHECK: Duration = Duration::from_secs(60 * 60);
/// The page the CLI asks to open, checked before the browser opens
/// (`is_login_url`).
const LOGIN_PATH: &str = "/loginDeepControl";
/// Anything the settings row shows changed (waiting, an error, the CLI's
/// support became known).
pub const EVENT: &str = "cursor-login-updated";
/// The kept sign-in itself changed (signed in, signed out, ended): the panel
/// reads the Cursor card again. Not sent for the rest, so opening Settings
/// never makes a hidden panel read quotas.
pub const CHANGED_EVENT: &str = "cursor-login-changed";

// --- Storage ------------------------------------------------------------------

/// The Credential Manager, or a stand-in in tests.
pub(crate) trait Vault {
    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, StoreError>;
    fn store(&self, target: &str, blob: &[u8]) -> Result<(), StoreError>;
    fn delete(&self, target: &str) -> Result<(), StoreError>;
}

struct SystemVault;

impl Vault for SystemVault {
    fn load(&self, target: &str) -> Result<Option<Vec<u8>>, StoreError> {
        credential_store::load(target)
    }
    fn store(&self, target: &str, blob: &[u8]) -> Result<(), StoreError> {
        credential_store::store(target, blob)
    }
    fn delete(&self, target: &str) -> Result<(), StoreError> {
        credential_store::delete(target)
    }
}

fn part_target(index: usize) -> String {
    if index == 0 {
        TARGET.to_string()
    } else {
        format!("{TARGET}/{}", index + 1)
    }
}

/// `<parts>:` then the first part in the main entry, the rest in `/2`…`/4`.
/// The extra parts are written first, so the main entry never points at
/// parts that are not there yet.
fn write_blob(vault: &dyn Vault, bytes: &[u8]) -> Result<(), StoreError> {
    let parts: Vec<&[u8]> = bytes.chunks(PART_BYTES).collect();
    if parts.is_empty() || parts.len() > MAX_PARTS {
        return Err(StoreError::Failed);
    }
    for index in (1..parts.len()).rev() {
        vault.store(&part_target(index), parts[index])?;
    }
    let mut head = format!("{}:", parts.len()).into_bytes();
    head.extend_from_slice(parts[0]);
    vault.store(TARGET, &head)?;
    for index in parts.len()..MAX_PARTS {
        let _ = vault.delete(&part_target(index));
    }
    Ok(())
}

fn read_blob(vault: &dyn Vault) -> Result<Option<Vec<u8>>, StoreError> {
    let Some(head) = vault.load(TARGET)? else {
        return Ok(None);
    };
    let (count, first) = match head.split_first() {
        Some((digit, rest)) if rest.first() == Some(&b':') => ((*digit as char).to_digit(10), &rest[1..]),
        _ => (None, &head[..0]),
    };
    let count = count.map(|n| n as usize).filter(|n| (1..=MAX_PARTS).contains(n)).ok_or(StoreError::Failed)?;
    let mut bytes = first.to_vec();
    for index in 1..count {
        bytes.extend(vault.load(&part_target(index))?.ok_or(StoreError::Failed)?);
    }
    Ok(Some(bytes))
}

/// The main entry goes first: a delete that stops half-way leaves no sign-in.
fn delete_blob(vault: &dyn Vault) -> Result<(), StoreError> {
    vault.delete(TARGET)?;
    for index in 1..MAX_PARTS {
        let _ = vault.delete(&part_target(index));
    }
    Ok(())
}

/// What the entry holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stored {
    None,
    SignedIn(Map<String, Value>),
    /// Cursor ended it: only the time and the email are kept.
    Expired { email: Option<String> },
    /// Present but unreadable; treated like an ended sign-in.
    Unreadable,
}

fn load_stored(vault: &dyn Vault) -> Stored {
    match read_blob(vault) {
        Ok(None) => Stored::None,
        Ok(Some(bytes)) => parse_stored(&bytes),
        Err(_) => Stored::Unreadable,
    }
}

fn parse_stored(bytes: &[u8]) -> Stored {
    let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(bytes) else {
        return Stored::Unreadable;
    };
    if map.get("rejectedAt").and_then(Value::as_str).is_some() {
        return Stored::Expired { email: email(&map) };
    }
    match credential(Value::Object(map)) {
        Some(map) => Stored::SignedIn(map),
        None => Stored::Unreadable,
    }
}

const CREDENTIAL_KEYS: [&str; 8] = [
    "version", "accessToken", "refreshToken", "userId", "obtainedAt", "expiresAt", "email", "refreshedAt",
];

fn clean_token(value: Option<&Value>) -> Option<&str> {
    let token = value?.as_str()?;
    (!token.is_empty() && token.len() <= 16_384 && !token.bytes().any(|b| b <= 0x20 || b == 0x7f)).then_some(token)
}

/// A sign-in record as the CLI prints it (`cursorCredential`), or None. Only
/// its known fields are kept; the refresh token is dropped when it is the
/// access token (Cursor renews with the access token; the CLI falls back to it).
fn credential(value: Value) -> Option<Map<String, Value>> {
    let Value::Object(raw) = value else { return None };
    let access = clean_token(raw.get("accessToken"))?.to_string();
    raw.get("userId").and_then(Value::as_str).filter(|id| !id.trim().is_empty())?;
    let mut map = Map::new();
    for key in CREDENTIAL_KEYS {
        if let Some(value) = raw.get(key) {
            map.insert(key.to_string(), value.clone());
        }
    }
    if clean_token(map.get("refreshToken")).is_none_or(|refresh| refresh == access) {
        map.remove("refreshToken");
    }
    Some(map)
}

fn access_token(map: &Map<String, Value>) -> &str {
    map.get("accessToken").and_then(Value::as_str).unwrap_or_default()
}

fn token_digest(map: &Map<String, Value>) -> String {
    hex::encode(Sha256::digest(access_token(map).as_bytes()))
}

fn email(map: &Map<String, Value>) -> Option<String> {
    let email = map.get("email")?.as_str()?.trim();
    (email.len() <= 320 && email.contains('@') && !email.chars().any(char::is_control)).then(|| email.to_string())
}

fn expires_at(map: &Map<String, Value>) -> Option<i64> {
    let text = map.get("expiresAt")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|date| date.timestamp())
}

/// Within a week of `expiresAt`. A sign-in that does not say when it ends is
/// renewed only after Cursor rejects it.
fn due(map: &Map<String, Value>, now: i64) -> bool {
    expires_at(map).is_some_and(|expires| expires - now <= REFRESH_LEAD_SECS)
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// What an ended sign-in leaves: no token, only when and which account.
fn ended_record(email: Option<String>) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("version".into(), 1.into());
    map.insert("rejectedAt".into(), chrono::Utc::now().to_rfc3339().into());
    if let Some(email) = email {
        map.insert("email".into(), email.into());
    }
    map
}

// --- State --------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginState {
    SignedOut,
    Waiting,
    SignedIn,
    Expired,
}

/// Why the last sign-in did not complete (the settings row's red line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginError {
    Timeout,
    Network,
    Proxy,
    Failed,
    BadUrl,
    SaveFailed,
    /// The system proxy changed while waiting for the browser.
    ProxyChanged,
    /// No Node.js (or Bun) to run the CLI with.
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub state: LoginState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Whether the CLI offers `login cursor`; `None` until known.
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<LoginError>,
}

fn view(stored: &Stored, waiting: bool, supported: Option<bool>, error: Option<LoginError>) -> LoginStatus {
    let (state, email) = match stored {
        Stored::None => (LoginState::SignedOut, None),
        Stored::SignedIn(map) => (LoginState::SignedIn, email(map)),
        Stored::Expired { email } => (LoginState::Expired, email.clone()),
        Stored::Unreadable => (LoginState::Expired, None),
    };
    LoginStatus {
        state: if waiting { LoginState::Waiting } else { state },
        email,
        supported,
        error: if waiting { None } else { error },
    }
}

#[derive(Default)]
pub struct CursorLogin {
    /// Every write of the entry, with `generation`, under this lock.
    writes: Mutex<()>,
    /// Bumped by each sign-in, sign-out, renewal and ending, so a renewal
    /// that finishes after one of them does not write over it.
    generation: AtomicU64,
    /// Renewals one at a time: each one adds a session on cursor.com.
    renewal: tokio::sync::Mutex<()>,
    flow: Mutex<Option<(u64, tauri::async_runtime::JoinHandle<()>)>>,
    flow_ids: AtomicU64,
    error: Mutex<Option<LoginError>>,
    held_until: Mutex<Option<Instant>>,
    /// The token the last renewal produced (a digest) and when, until a read
    /// with it went through. Rejected before that (within `FRESH_WINDOW`),
    /// the sign-in has ended: renewing again would only add another session
    /// on cursor.com.
    fresh_from_renewal: Mutex<Option<(String, Instant)>>,
    supported: Mutex<Option<(bool, Instant)>>,
    checking_support: AtomicBool,
}

#[derive(Debug, PartialEq, Eq)]
enum Write {
    Done,
    /// Signed in, out or renewed meanwhile: nothing written.
    Stale,
    Failed,
}

impl CursorLogin {
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// `value` (None deletes) unless the generation moved since `expected`.
    fn write(&self, vault: &dyn Vault, expected: Option<u64>, value: Option<&Map<String, Value>>) -> Write {
        let _guard = self.writes.lock().unwrap();
        if expected.is_some_and(|generation| generation != self.generation()) {
            return Write::Stale;
        }
        let result = match value {
            Some(map) => serde_json::to_vec(map).map_err(|_| StoreError::Failed).and_then(|bytes| write_blob(vault, &bytes)),
            None => delete_blob(vault),
        };
        self.generation.fetch_add(1, Ordering::SeqCst);
        if result.is_ok() { Write::Done } else { Write::Failed }
    }

    fn held(&self) -> bool {
        self.held_until.lock().unwrap().is_some_and(|until| Instant::now() < until)
    }

    fn hold(&self) {
        *self.held_until.lock().unwrap() = Some(Instant::now() + REFRESH_HOLD);
    }

    fn supported(&self) -> Option<bool> {
        self.supported.lock().unwrap().map(|(supported, _)| supported)
    }

    fn reset_after_sign_in_or_out(&self) {
        *self.error.lock().unwrap() = None;
        *self.held_until.lock().unwrap() = None;
        *self.fresh_from_renewal.lock().unwrap() = None;
    }

    fn mark_fresh(&self, map: &Map<String, Value>) {
        *self.fresh_from_renewal.lock().unwrap() = Some((token_digest(map), Instant::now()));
    }

    fn is_fresh(&self, map: &Map<String, Value>) -> bool {
        self.fresh_from_renewal
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(digest, at)| *digest == token_digest(map) && at.elapsed() < FRESH_WINDOW)
    }
}

pub fn status(app: &AppHandle) -> LoginStatus {
    let login = &app.state::<AppCtx>().cursor_login;
    let waiting = login.flow.lock().unwrap().is_some();
    let error = *login.error.lock().unwrap();
    view(&load_stored(&SystemVault), waiting, login.supported(), error)
}

fn emit_status(app: &AppHandle) {
    let _ = app.emit(EVENT, status(app));
}

/// The sign-in changed: the Cursor card is read again (with the new state),
/// and both windows hear of it.
fn after_change(app: &AppHandle) {
    app.state::<AppCtx>().rate_limits.lock().unwrap().invalidate_cursor();
    emit_status(app);
    let _ = app.emit(CHANGED_EVENT, ());
}

// --- What a CLI run gets ----------------------------------------------------------

/// What a `sync` or `quota fetch` gets on stdin. `None` (no flag, no stdin)
/// when the app keeps no sign-in: the CLI then reads Cursor's own local login
/// as before.
// No `Debug`: it holds the sign-in.
#[derive(Clone)]
pub struct Handover {
    pub input: Vec<u8>,
    credential: Option<Map<String, Value>>,
}

impl Handover {
    fn live(map: Map<String, Value>) -> Self {
        Self { input: serde_json::to_vec(&map).unwrap_or_default(), credential: Some(map) }
    }

    /// No token: the CLI reads it as an unusable sign-in, reports `signIn:
    /// expired` and sends nothing for Cursor -- as with its own ended file,
    /// it does not fall back to Cursor's local login.
    fn ended() -> Self {
        Self { input: serde_json::to_vec(&ended_record(None)).unwrap_or_default(), credential: None }
    }

    pub fn is_live(&self) -> bool {
        self.credential.is_some()
    }

    fn of(stored: Stored) -> Option<Self> {
        match stored {
            Stored::None => None,
            Stored::SignedIn(map) => Some(Self::live(map)),
            Stored::Expired { .. } | Stored::Unreadable => Some(Self::ended()),
        }
    }
}

/// The sign-in to hand over, renewed first when it is within a week of its
/// end (one renewal at a time; after a temporary failure not again for 6
/// hours, and the current one is used while it lasts).
pub async fn for_cli(app: &AppHandle) -> Option<Handover> {
    let ctx = app.state::<AppCtx>();
    let login = &ctx.cursor_login;
    let _renewal = login.renewal.lock().await;
    let generation = login.generation();
    let map = match load_stored(&SystemVault) {
        Stored::SignedIn(map) => map,
        other => return Handover::of(other),
    };
    if !due(&map, now_secs()) || login.held() {
        return Some(Handover::live(map));
    }
    match refresh(app, &map, generation).await {
        Refresh::Renewed(next) => match login.write(&SystemVault, Some(generation), Some(&next)) {
            // Not saved: this run still uses it; the old one keeps working.
            Write::Done | Write::Failed => {
                login.mark_fresh(&next);
                Some(Handover::live(next))
            }
            Write::Stale => Handover::of(load_stored(&SystemVault)),
        },
        Refresh::Ended => {
            end(app, generation, &map);
            Handover::of(load_stored(&SystemVault))
        }
        // Signed in or out meanwhile: the current state decides, and the
        // hold is not put on a sign-in it was not about.
        Refresh::Retry if login.generation() != generation => Handover::of(load_stored(&SystemVault)),
        Refresh::Retry => {
            login.hold();
            Some(Handover::live(map))
        }
    }
}

/// Right before a CLI starts, once its command is ready (the proxy is read
/// after any renewal `for_cli` waited for): the sign-in as kept now. A
/// sign-out, a new sign-in or an ending meanwhile wins over `handover`.
pub fn recheck(handover: Option<Handover>) -> Option<Handover> {
    let current = Handover::of(load_stored(&SystemVault));
    match (&handover, &current) {
        (Some(given), Some(now)) if given.credential.is_some() && given.credential == now.credential => handover,
        _ => current,
    }
}

/// Cursor accepted the sign-in in `handover` (a live read went through).
pub fn read_ok(app: &AppHandle, handover: &Handover) {
    let Some(map) = handover.credential.as_ref() else { return };
    let login = &app.state::<AppCtx>().cursor_login;
    let mut fresh = login.fresh_from_renewal.lock().unwrap();
    if fresh.as_ref().is_some_and(|(digest, _)| *digest == token_digest(map)) {
        *fresh = None;
    }
}

/// What to do after the CLI said a handed-over sign-in has ended
/// (`signIn: expired`, or the sync's notice).
pub enum AfterRejection {
    /// Renewed (or renewed meanwhile): read again with this one.
    Renewed(Handover),
    /// Cursor ended it; the entry now says so.
    Ended,
    /// The renewal did not complete for a temporary reason.
    Retry,
    /// Signed out meanwhile.
    Gone,
}

/// Cursor rejected `passed` (or it lapsed): renew once and read again; a
/// renewal Cursor refuses, or a renewed sign-in rejected again, ends it.
pub async fn after_rejection(app: &AppHandle, passed: &Handover) -> AfterRejection {
    let Some(rejected) = passed.credential.as_ref() else {
        return AfterRejection::Ended;
    };
    let ctx = app.state::<AppCtx>();
    let login = &ctx.cursor_login;
    let _renewal = login.renewal.lock().await;
    let generation = login.generation();
    let map = match load_stored(&SystemVault) {
        Stored::SignedIn(map) => map,
        Stored::None => return AfterRejection::Gone,
        Stored::Expired { .. } | Stored::Unreadable => return AfterRejection::Ended,
    };
    if access_token(&map) != access_token(rejected) {
        return AfterRejection::Renewed(Handover::live(map));
    }
    if login.is_fresh(&map) {
        end(app, generation, &map);
        return AfterRejection::Ended;
    }
    if login.held() {
        return AfterRejection::Retry;
    }
    match refresh(app, &map, generation).await {
        Refresh::Renewed(next) => match login.write(&SystemVault, Some(generation), Some(&next)) {
            Write::Done | Write::Failed => {
                login.mark_fresh(&next);
                AfterRejection::Renewed(Handover::live(next))
            }
            Write::Stale => current_after_change(),
        },
        Refresh::Ended => {
            end(app, generation, &map);
            AfterRejection::Ended
        }
        Refresh::Retry if login.generation() != generation => current_after_change(),
        Refresh::Retry => {
            login.hold();
            AfterRejection::Retry
        }
    }
}

/// Signed in, out or ended while a renewal ran: answer from what is kept now.
fn current_after_change() -> AfterRejection {
    match Handover::of(load_stored(&SystemVault)) {
        Some(handover) if handover.is_live() => AfterRejection::Renewed(handover),
        Some(_) => AfterRejection::Ended,
        None => AfterRejection::Gone,
    }
}

/// Cursor ended the sign-in: drop the tokens, keep the email for the row.
fn end(app: &AppHandle, generation: u64, map: &Map<String, Value>) {
    let login = &app.state::<AppCtx>().cursor_login;
    if login.write(&SystemVault, Some(generation), Some(&ended_record(email(map)))) != Write::Stale {
        after_change(app);
    }
}

// --- Renewal ------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Refresh {
    Renewed(Map<String, Value>),
    Ended,
    Retry,
}

/// Renews `credential`, kept as of `generation`. Nothing starts when the user
/// signed in or out while the command was prepared (reading the proxy, PAC
/// included, can take seconds); the caller then answers from what is kept.
async fn refresh(app: &AppHandle, credential: &Map<String, Value>, generation: u64) -> Refresh {
    let Ok(input) = serde_json::to_vec(credential) else { return Refresh::Retry };
    let Ok(mut command) = sync_engine::cursor_login_command(
        app,
        &["login", "cursor", "--refresh", "--json", "--cursor-login-stdin"],
    ) else {
        return Refresh::Retry;
    };
    if app.state::<AppCtx>().cursor_login.generation() != generation {
        return Refresh::Retry;
    }
    // Never kept: the renewed sign-in comes back on stdout.
    command.stderr(Stdio::null());
    match tokio::time::timeout(REFRESH_TIMEOUT, crate::process_lifecycle::output_with_input(&mut command, &input)).await {
        Ok(Ok(output)) => parse_refresh_output(&output.stdout),
        _ => Refresh::Retry,
    }
}

/// `{"event":"credential",…}` / `{"event":"expired"}` / `{"event":"error",…}`.
fn parse_refresh_output(stdout: &[u8]) -> Refresh {
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else { continue };
        match value.get("event").and_then(Value::as_str) {
            Some("credential") => {
                return value.get("credential").cloned().and_then(credential).map_or(Refresh::Retry, Refresh::Renewed);
            }
            Some("expired") => return Refresh::Ended,
            Some(_) => return Refresh::Retry,
            None => {}
        }
    }
    Refresh::Retry
}

// --- Signing in ------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum LoginEvent {
    Open(String),
    Credential(Map<String, Value>),
    Error(LoginError),
}

fn parse_login_event(line: &str) -> Option<LoginEvent> {
    let value = serde_json::from_str::<Value>(line.trim()).ok()?;
    Some(match value.get("event")?.as_str()? {
        "open" => LoginEvent::Open(value.get("url")?.as_str()?.to_string()),
        "credential" => match value.get("credential").cloned().and_then(credential) {
            Some(map) => LoginEvent::Credential(map),
            None => LoginEvent::Error(LoginError::Failed),
        },
        "error" => LoginEvent::Error(match value.get("code").and_then(Value::as_str) {
            Some("timeout") => LoginError::Timeout,
            Some("network") => LoginError::Network,
            Some("proxy") => LoginError::Proxy,
            _ => LoginError::Failed,
        }),
        _ => return None,
    })
}

/// Exactly `https://cursor.com/loginDeepControl?…`: nothing else is opened.
fn is_login_url(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else { return false };
    url.scheme() == "https"
        && url.host_str() == Some("cursor.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == LOGIN_PATH
        && url.fragment().is_none()
}

/// Starts signing in (one at a time). The browser opens once the CLI has
/// prepared the sign-in and its URL passed `is_login_url`.
pub fn start(app: &AppHandle) -> LoginStatus {
    let login = &app.state::<AppCtx>().cursor_login;
    {
        let mut flow = login.flow.lock().unwrap();
        if flow.is_none() {
            *login.error.lock().unwrap() = None;
            let id = login.flow_ids.fetch_add(1, Ordering::SeqCst);
            let task_app = app.clone();
            *flow = Some((id, tauri::async_runtime::spawn(async move { run_login(task_app, id).await })));
        }
    }
    let current = status(app);
    let _ = app.emit(EVENT, &current);
    current
}

/// Stops a sign-in in progress (its CLI and anything it started).
pub fn cancel(app: &AppHandle) -> LoginStatus {
    let login = &app.state::<AppCtx>().cursor_login;
    if let Some((_, handle)) = login.flow.lock().unwrap().take() {
        handle.abort();
    }
    let current = status(app);
    let _ = app.emit(EVENT, &current);
    current
}

/// Forgets the sign-in. Cursor has no public sign-out: the session stays
/// listed on cursor.com until it lapses or the user revokes it there.
pub fn logout(app: &AppHandle) -> Result<LoginStatus, String> {
    let login = &app.state::<AppCtx>().cursor_login;
    if let Some((_, handle)) = login.flow.lock().unwrap().take() {
        handle.abort();
    }
    if login.write(&SystemVault, None, None) == Write::Failed {
        return Err("无法删除 Windows 凭据管理器中的 Cursor 登录".into());
    }
    login.reset_after_sign_in_or_out();
    after_change(app);
    Ok(status(app))
}

enum Outcome {
    Credential(Map<String, Value>),
    Error(LoginError),
}

async fn run_login(app: AppHandle, id: u64) {
    let outcome = tokio::time::timeout(LOGIN_DEADLINE, sign_in(&app))
        .await
        .unwrap_or(Outcome::Error(LoginError::Timeout));
    let login = &app.state::<AppCtx>().cursor_login;
    let mut signed_in = false;
    {
        let mut flow = login.flow.lock().unwrap();
        // Cancelled meanwhile: this attempt no longer counts.
        if flow.as_ref().is_none_or(|(current, _)| *current != id) {
            return;
        }
        match outcome {
            Outcome::Credential(map) => {
                if login.write(&SystemVault, None, Some(&map)) == Write::Done {
                    login.reset_after_sign_in_or_out();
                    signed_in = true;
                } else {
                    *login.error.lock().unwrap() = Some(LoginError::SaveFailed);
                }
            }
            Outcome::Error(error) => *login.error.lock().unwrap() = Some(error),
        }
        *flow = None;
    }
    if signed_in {
        after_change(&app);
    } else {
        emit_status(&app);
    }
    if signed_in {
        // Read Cursor now; the CLI fills the weeks it skipped.
        crate::services::sync_engine::run_sync(app.clone(), crate::services::sync_history::SyncTrigger::Manual).await;
    }
}

/// The routes the system gives the sign-in's hosts now (manual proxy or PAC),
/// or None when the app's own proxy variables decide (they do not change).
fn sign_in_routes() -> Option<Vec<(String, crate::system_proxy::Route)>> {
    let lookup = |name: &str| std::env::var(name).ok();
    if crate::system_proxy::has_own_proxy(&lookup) {
        return None;
    }
    Some(crate::system_proxy::live_routes(&crate::system_proxy::host_urls(&SIGN_IN_HOSTS)))
}

/// The system now sends one of the sign-in's hosts another way (a proxy
/// switched on or off, or another one). A PAC that cannot answer right now is
/// no change: the CLI keeps what it was given, which never goes direct.
fn route_changed(
    before: &[(String, crate::system_proxy::Route)],
    now: &[(String, crate::system_proxy::Route)],
) -> bool {
    before.len() != now.len()
        || before
            .iter()
            .zip(now)
            .any(|((_, was), (_, is))| *is != crate::system_proxy::Route::Unresolved && was != is)
}

async fn sign_in(app: &AppHandle) -> Outcome {
    use tokio::io::AsyncBufReadExt;
    let Ok(mut command) = sync_engine::cursor_login_command(app, &["login", "cursor", "--json"]) else {
        // The launcher is part of the app; what can be missing is Node.js.
        return Outcome::Error(if sync_engine::cli_entry(app).is_some() { LoginError::Runtime } else { LoginError::Failed });
    };
    // The CLI follows the proxy it was started with for up to 5 minutes. The
    // user may switch theirs on while the browser asks (Cursor's page did not
    // load, say); then this attempt stops instead of going on the old way.
    let routes = tauri::async_runtime::spawn_blocking(sign_in_routes).await.ok().flatten();
    // Never kept: the sign-in comes back on stdout.
    command.stdin(Stdio::null()).stderr(Stdio::null());
    let Ok((mut child, _guard)) = crate::process_lifecycle::spawn(&mut command) else {
        return Outcome::Error(LoginError::Failed);
    };
    let Some(stdout) = child.stdout.take() else {
        return Outcome::Error(LoginError::Failed);
    };
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let mut opened = false;
    let mut check = tokio::time::interval(ROUTE_CHECK);
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    check.tick().await;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                match parse_login_event(&line) {
                    Some(LoginEvent::Open(url)) if !opened => {
                        if !is_login_url(&url) {
                            return Outcome::Error(LoginError::BadUrl);
                        }
                        if crate::process_utils::shell_open(&url).is_err() {
                            return Outcome::Error(LoginError::Failed);
                        }
                        opened = true;
                    }
                    Some(LoginEvent::Credential(map)) if opened => return Outcome::Credential(map),
                    Some(LoginEvent::Error(error)) => return Outcome::Error(error),
                    _ => {}
                }
            }
            _ = check.tick(), if routes.is_some() => {
                let now = tauri::async_runtime::spawn_blocking(sign_in_routes).await.ok().flatten();
                if let (Some(before), Some(now)) = (routes.as_deref(), now.as_deref()) {
                    if route_changed(before, now) {
                        return Outcome::Error(LoginError::ProxyChanged);
                    }
                }
            }
        }
    }
    // Returning drops the child and its job: nothing it started keeps running.
    Outcome::Error(LoginError::Failed)
}

// --- Does the CLI offer it -----------------------------------------------------------

/// Asks the CLI once whether it offers `login cursor` (its `help --all`), in
/// the background; the answer arrives with `EVENT`. A yes is kept; a no is
/// asked again after an hour; a failed check, at the next call.
pub fn check_support(app: &AppHandle) {
    let login = &app.state::<AppCtx>().cursor_login;
    let fresh = login
        .supported
        .lock()
        .unwrap()
        .is_some_and(|(supported, at)| supported || at.elapsed() < UNSUPPORTED_RECHECK);
    if fresh || login.checking_support.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let answer = ask_support(&app).await;
        let login = &app.state::<AppCtx>().cursor_login;
        if let Some(supported) = answer {
            *login.supported.lock().unwrap() = Some((supported, Instant::now()));
        }
        login.checking_support.store(false, Ordering::SeqCst);
        emit_status(&app);
    });
}

async fn ask_support(app: &AppHandle) -> Option<bool> {
    let mut command = sync_engine::cursor_login_command(app, &["help", "--all"]).ok()?;
    command.stdin(Stdio::null()).stderr(Stdio::null());
    let output = tokio::time::timeout(SUPPORT_TIMEOUT, crate::process_lifecycle::output(&mut command))
        .await
        .ok()?
        .ok()?;
    output.status.success().then(|| offers_login(&output.stdout))
}

fn offers_login(help: &[u8]) -> bool {
    String::from_utf8_lossy(help).contains("login cursor")
}

// --- Tests --------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryVault {
        entries: RefCell<HashMap<String, Vec<u8>>>,
        refuse_writes: bool,
    }

    impl Vault for MemoryVault {
        fn load(&self, target: &str) -> Result<Option<Vec<u8>>, StoreError> {
            Ok(self.entries.borrow().get(target).cloned())
        }
        fn store(&self, target: &str, blob: &[u8]) -> Result<(), StoreError> {
            if self.refuse_writes || blob.len() > credential_store::MAX_BLOB {
                return Err(StoreError::Failed);
            }
            self.entries.borrow_mut().insert(target.to_string(), blob.to_vec());
            Ok(())
        }
        fn delete(&self, target: &str) -> Result<(), StoreError> {
            self.entries.borrow_mut().remove(target);
            Ok(())
        }
    }

    fn jwt_like(len: usize) -> String {
        "a".repeat(len)
    }

    fn record(access: &str, refresh: &str) -> Value {
        serde_json::json!({
            "version": 1, "accessToken": access, "refreshToken": refresh, "userId": "user_01",
            "obtainedAt": "2026-11-25T12:11:00.000Z", "expiresAt": "2027-01-24T12:11:00.000Z",
            "email": "someone@example.com", "unexpected": "dropped",
        })
    }

    #[test]
    fn a_credential_keeps_only_known_fields_and_drops_a_duplicate_refresh_token() {
        let same = credential(record("tok", "tok")).unwrap();
        assert!(!same.contains_key("refreshToken"));
        assert!(!same.contains_key("unexpected"));
        assert_eq!(access_token(&same), "tok");
        let different = credential(record("tok", "other")).unwrap();
        assert_eq!(different.get("refreshToken").and_then(Value::as_str), Some("other"));
        // Not a sign-in: no token, a token with spaces, or no user id.
        assert!(credential(serde_json::json!({"userId": "u"})).is_none());
        assert!(credential(serde_json::json!({"accessToken": "a b", "userId": "u"})).is_none());
        assert!(credential(serde_json::json!({"accessToken": "a"})).is_none());
        assert!(credential(serde_json::json!("text")).is_none());
    }

    #[test]
    fn long_sign_ins_are_split_across_entries_and_read_back_whole() {
        let vault = MemoryVault::default();
        let login = CursorLogin::default();
        // Two long JWTs: more than one 2560-byte entry holds.
        let map = credential(record(&jwt_like(1900), &format!("r{}", jwt_like(1900)))).unwrap();
        assert_eq!(login.write(&vault, None, Some(&map)), Write::Done);
        assert!(vault.entries.borrow().len() >= 2);
        assert_eq!(load_stored(&vault), Stored::SignedIn(map));
        // A shorter one afterwards leaves no stale parts behind.
        let short = credential(record("tok", "tok")).unwrap();
        assert_eq!(login.write(&vault, None, Some(&short)), Write::Done);
        assert_eq!(vault.entries.borrow().len(), 1);
        assert_eq!(load_stored(&vault), Stored::SignedIn(short));
        // Sign-out removes everything.
        assert_eq!(login.write(&vault, None, None), Write::Done);
        assert!(vault.entries.borrow().is_empty());
        assert_eq!(load_stored(&vault), Stored::None);
    }

    #[test]
    fn a_missing_part_or_a_damaged_entry_reads_as_unusable() {
        let vault = MemoryVault::default();
        let login = CursorLogin::default();
        let map = credential(record(&jwt_like(1900), &format!("r{}", jwt_like(1900)))).unwrap();
        login.write(&vault, None, Some(&map));
        vault.entries.borrow_mut().remove(&part_target(1));
        assert_eq!(load_stored(&vault), Stored::Unreadable);
        vault.entries.borrow_mut().insert(TARGET.into(), b"not a header".to_vec());
        assert_eq!(load_stored(&vault), Stored::Unreadable);
        vault.entries.borrow_mut().insert(TARGET.into(), b"1:{not json".to_vec());
        assert_eq!(load_stored(&vault), Stored::Unreadable);
    }

    #[test]
    fn an_ended_sign_in_keeps_no_token() {
        let vault = MemoryVault::default();
        let login = CursorLogin::default();
        let ended = ended_record(Some("someone@example.com".into()));
        login.write(&vault, None, Some(&ended));
        assert_eq!(load_stored(&vault), Stored::Expired { email: Some("someone@example.com".into()) });
        assert!(!String::from_utf8(read_blob(&vault).unwrap().unwrap()).unwrap().contains("accessToken"));
        // What the CLI gets for it carries no token either.
        let handover = Handover::of(load_stored(&vault)).unwrap();
        assert!(!handover.is_live());
        assert!(!String::from_utf8_lossy(&handover.input).contains("Token"));
    }

    #[test]
    fn a_write_after_a_sign_in_or_out_meanwhile_is_dropped() {
        let vault = MemoryVault::default();
        let login = CursorLogin::default();
        let first = credential(record("first", "first")).unwrap();
        login.write(&vault, None, Some(&first));
        let started = login.generation();
        // The user signs out while a renewal is in flight.
        login.write(&vault, None, None);
        let renewed = credential(record("renewed", "renewed")).unwrap();
        assert_eq!(login.write(&vault, Some(started), Some(&renewed)), Write::Stale);
        assert_eq!(load_stored(&vault), Stored::None);
    }

    #[test]
    fn a_refused_write_is_reported() {
        let vault = MemoryVault { refuse_writes: true, ..Default::default() };
        let login = CursorLogin::default();
        assert_eq!(login.write(&vault, None, Some(&credential(record("t", "t")).unwrap())), Write::Failed);
    }

    #[test]
    fn renewal_is_due_a_week_before_the_end_only() {
        let map = credential(record("t", "t")).unwrap();
        let expires = expires_at(&map).unwrap();
        assert!(!due(&map, expires - REFRESH_LEAD_SECS - 1));
        assert!(due(&map, expires - REFRESH_LEAD_SECS));
        assert!(due(&map, expires + 1));
        let mut undated = map.clone();
        undated.remove("expiresAt");
        assert!(!due(&undated, expires));
    }

    #[test]
    fn only_cursors_own_sign_in_page_is_opened() {
        assert!(is_login_url(
            "https://cursor.com/loginDeepControl?challenge=abc&uuid=1-2&mode=login&redirectTarget=cli"
        ));
        for url in [
            "http://cursor.com/loginDeepControl?challenge=abc",
            "https://cursor.com.evil.example/loginDeepControl",
            "https://evil.example/loginDeepControl",
            "https://www.cursor.com/loginDeepControl",
            "https://cursor.com:8443/loginDeepControl",
            "https://user@cursor.com/loginDeepControl",
            "https://cursor.com/loginDeepControl/../settings",
            "https://cursor.com/settings",
            "https://cursor.com/loginDeepControl#x",
            "file:///C:/Windows/System32/calc.exe",
            "not a url",
        ] {
            assert!(!is_login_url(url), "{url}");
        }
    }

    #[test]
    fn sign_in_output_maps_to_fixed_events() {
        assert_eq!(
            parse_login_event(r#"{"event":"open","url":"https://cursor.com/loginDeepControl?x=1"}"#),
            Some(LoginEvent::Open("https://cursor.com/loginDeepControl?x=1".into()))
        );
        let line = serde_json::json!({"event": "credential", "credential": record("tok", "tok")}).to_string();
        assert!(matches!(parse_login_event(&line), Some(LoginEvent::Credential(_))));
        assert_eq!(
            parse_login_event(r#"{"event":"credential","credential":{"accessToken":""}}"#),
            Some(LoginEvent::Error(LoginError::Failed))
        );
        for (code, error) in [
            ("timeout", LoginError::Timeout),
            ("network", LoginError::Network),
            ("proxy", LoginError::Proxy),
            ("failed", LoginError::Failed),
            ("usage", LoginError::Failed),
        ] {
            let line = format!(r#"{{"event":"error","code":"{code}","message":"free text never kept"}}"#);
            assert_eq!(parse_login_event(&line), Some(LoginEvent::Error(error)));
        }
        assert_eq!(parse_login_event("请在浏览器里确认登录 Cursor"), None);
        assert_eq!(parse_login_event(r#"{"event":"somethingNew"}"#), None);
    }

    #[test]
    fn renewal_output_maps_to_renewed_ended_or_retry() {
        let line = serde_json::json!({"event": "credential", "credential": record("new", "new")}).to_string();
        assert!(matches!(parse_refresh_output(line.as_bytes()), Refresh::Renewed(map) if access_token(&map) == "new"));
        assert_eq!(parse_refresh_output(br#"{"event":"expired"}"#), Refresh::Ended);
        assert_eq!(parse_refresh_output(br#"{"event":"error","code":"retryable","message":"x"}"#), Refresh::Retry);
        assert_eq!(parse_refresh_output(br#"{"event":"error","code":"proxy","message":"x"}"#), Refresh::Retry);
        assert_eq!(parse_refresh_output(b""), Refresh::Retry);
        assert_eq!(parse_refresh_output(b"garbage\n"), Refresh::Retry);
    }

    #[test]
    fn the_row_shows_the_stored_state_and_the_last_error() {
        let signed_in = Stored::SignedIn(credential(record("t", "t")).unwrap());
        let status = view(&signed_in, false, Some(true), None);
        assert_eq!(status.state, LoginState::SignedIn);
        assert_eq!(status.email.as_deref(), Some("someone@example.com"));
        assert_eq!(view(&Stored::None, false, Some(true), Some(LoginError::Timeout)).error, Some(LoginError::Timeout));
        // Waiting hides an earlier error, also while signing in again after expiry.
        let waiting = view(&Stored::Expired { email: None }, true, Some(true), Some(LoginError::Network));
        assert_eq!(waiting.state, LoginState::Waiting);
        assert_eq!(waiting.error, None);
        assert_eq!(view(&Stored::Unreadable, false, None, None).state, LoginState::Expired);
        let json = serde_json::to_value(view(&Stored::None, false, None, None)).unwrap();
        assert_eq!(json, serde_json::json!({"state": "signedOut", "supported": null}));
    }

    #[test]
    fn a_new_route_for_a_sign_in_host_stops_the_attempt() {
        use crate::system_proxy::Route;
        let routes = |api: Route, web: Route| vec![("api2.cursor.sh".to_string(), api), ("cursor.com".to_string(), web)];
        let direct = routes(Route::Direct, Route::Direct);
        let proxied = routes(Route::Http("http://127.0.0.1:7890".into()), Route::Http("http://127.0.0.1:7890".into()));
        assert!(!route_changed(&direct, &direct));
        // Switched on, off, or to another proxy, for either host.
        assert!(route_changed(&direct, &proxied));
        assert!(route_changed(&proxied, &direct));
        assert!(route_changed(&direct, &routes(Route::Direct, Route::Socks("127.0.0.1:7891".into()))));
        assert!(route_changed(&proxied, &routes(Route::Http("http://127.0.0.1:7890".into()), Route::Http("http://10.0.0.2:3128".into()))));
        // A PAC that cannot answer this moment is not a change.
        assert!(!route_changed(&proxied, &routes(Route::Unresolved, Route::Unresolved)));
    }

    #[test]
    fn support_is_read_from_the_full_help() {
        assert!(offers_login(b"    npx @vibe-cafe/vibe-usage login cursor  Sign in to Cursor for Vibe Usage"));
        assert!(!offers_login(b"    npx @vibe-cafe/vibe-usage quota fetch --product <id> --json"));
    }

    /// Native Credential Manager round trip with a dummy sign-in under a
    /// disposable target prefix (never the app's own entry). Runs in CI.
    #[cfg(windows)]
    #[test]
    fn credential_manager_keeps_a_split_sign_in() {
        struct Prefixed(String);
        impl Vault for Prefixed {
            fn load(&self, target: &str) -> Result<Option<Vec<u8>>, StoreError> {
                credential_store::load(&format!("{}{target}", self.0))
            }
            fn store(&self, target: &str, blob: &[u8]) -> Result<(), StoreError> {
                credential_store::store(&format!("{}{target}", self.0), blob)
            }
            fn delete(&self, target: &str) -> Result<(), StoreError> {
                credential_store::delete(&format!("{}{target}", self.0))
            }
        }
        let vault = Prefixed(format!(
            "ai.vibecafe.vibe-usage.acceptance/{}/{}/",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        struct Cleanup<'a>(&'a Prefixed);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = delete_blob(self.0);
            }
        }
        let _cleanup = Cleanup(&vault);
        let login = CursorLogin::default();
        let map = credential(record(&jwt_like(1900), &format!("r{}", jwt_like(1900)))).unwrap();
        assert_eq!(login.write(&vault, None, Some(&map)), Write::Done);
        assert_eq!(load_stored(&vault), Stored::SignedIn(map));
        assert_eq!(login.write(&vault, None, None), Write::Done);
        assert_eq!(load_stored(&vault), Stored::None);
    }
}
