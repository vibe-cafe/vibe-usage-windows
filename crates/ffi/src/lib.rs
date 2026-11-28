//! Process-wide native host. See native/contract/README.md for ABI ownership.
mod dispatch;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    ffi::{c_char, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    ptr,
    sync::{Arc, Mutex},
    time::Duration,
};
use vibe_backend::{
    serde::Deserialize,
    serde_json::{self, json, Value},
    tokio::{self, runtime::Runtime, sync::mpsc, task::JoinSet},
    Backend, Host, Options,
};

// A smoke host accidentally pointed at a production library must fail the
// handshake before it can start discovery, updates or credential access.
const VERSION: u32 = if cfg!(any(test, feature = "smoke")) {
    0x8000_0001
} else {
    1
};
type Callback = extern "C" fn(*mut c_char);
type Reply<T> = Result<T, Error>;

#[derive(Clone, Debug)]
enum Error {
    Version,
    Arguments,
    Unknown,
    Closed,
    AlreadyOpen,
    RequestId,
    CloseFromRuntime,
    Cancelled,
    Panic,
    Internal,
    Shutdown,
    Service(String),
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Service(message)
    }
}

impl Error {
    fn json(&self) -> Value {
        let (code, message) = match self {
            Self::Version => ("versionMismatch", "外壳和后端的接口版本不匹配"),
            Self::Arguments => ("invalidArguments", "命令参数或初始化参数无效"),
            Self::Unknown => ("unknownCommand", "后端不支持这个命令"),
            Self::Closed => ("closed", "后端未启动或已经关闭"),
            Self::AlreadyOpen => ("alreadyOpen", "后端已经启动"),
            Self::RequestId => ("invalidRequestId", "请求号必须递增且大于零"),
            Self::CloseFromRuntime => ("closeFromRuntime", "请在运行时线程以外关闭后端"),
            Self::Cancelled => ("cancelled", "请求已取消"),
            Self::Panic => ("panic", "后端任务异常终止"),
            Self::Internal => ("internal", "无法运行后端任务"),
            Self::Shutdown => ("shutdown", "未能确认后台子进程已退出"),
            Self::Service(reason) => ("backend", reason.as_str()),
        };
        json!({"code":code,"message":message})
    }
}

fn owned(value: Value) -> *mut c_char {
    // JSON escapes embedded NULs.
    CString::new(value.to_string()).unwrap().into_raw()
}

fn boundary(f: impl FnOnce() -> Reply<()>) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(Error::Panic)) {
        Ok(()) => ptr::null_mut(),
        Err(e) => owned(e.json()),
    }
}

unsafe fn input<'a>(p: *const c_char) -> Reply<&'a str> {
    if p.is_null() {
        return Err(Error::Arguments);
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| Error::Arguments)
}

struct Output(Callback);
impl Host for Output {
    fn rate_limits_updated(&self, payload: Value) {
        self.event("rate-limits-updated", payload);
    }
    fn event(&self, name: &str, payload: Value) {
        (self.0)(owned(json!({"kind":"event","name":name,"payload":payload})));
    }
    fn request_exit(&self) {
        self.event("request-exit", Value::Null);
    }
}

struct Call {
    id: u64,
    name: String,
    args: String,
}

async fn execute(app: Backend, call: Call) -> Reply<Value> {
    #[cfg(any(test, feature = "smoke"))]
    match call.name.as_str() {
        "__panic" => panic!("synthetic task panic"),
        "__pending" => return std::future::pending().await,
        "get_settings" | "get_sync_state" | "get_update_info" | "get_sync_history" => (),
        "update_setting"
            if serde_json::from_str::<Value>(&call.args)
                .ok()
                .and_then(|v| v.pointer("/change/showCostInTray").and_then(Value::as_bool))
                .is_some() =>
        {
            ()
        }
        _ => return Err(Error::Unknown),
    }
    dispatch::run(app, &call.name, &call.args).await
}

async fn serve(
    app: Backend,
    output: Arc<Output>,
    mut incoming: mpsc::UnboundedReceiver<Call>,
) -> Reply<()> {
    let mut tasks = JoinSet::new();
    let mut ids = HashMap::new();
    let mut closing = false;
    while !closing || !tasks.is_empty() {
        tokio::select! {
            call = incoming.recv(), if !closing => match call {
                Some(call) => {
                    let id = call.id;
                    let task = tasks.spawn(execute(app.clone(), call));
                    ids.insert(task.id(), id);
                }
                None => { closing = true; tasks.abort_all(); }
            },
            Some(result) = tasks.join_next_with_id(), if !tasks.is_empty() => {
                // Tokio catches a panic at the task boundary, including panics
                // after an await. Even aborted tasks keep their join ID.
                let (task_id, result) = match result {
                    Ok(done) => done,
                    Err(e) => (e.id(), Err(if e.is_panic() { Error::Panic } else { Error::Cancelled })),
                };
                let id = ids.remove(&task_id).unwrap();
                let envelope = match result {
                    Ok(value) => json!({"kind":"result","requestId":id,"value":value}),
                    Err(e) => json!({"kind":"result","requestId":id,"error":e.json()}),
                };
                (output.0)(owned(envelope));
            }
        }
    }
    if app.shutdown().await {
        Ok(())
    } else {
        Err(Error::Shutdown)
    }
}

struct Session {
    runtime: Runtime,
    incoming: mpsc::UnboundedSender<Call>,
    worker: tokio::task::JoinHandle<Reply<()>>,
    last_id: u64,
}
enum State {
    New,
    Open(Session),
    Closed(Reply<()>),
}
static STATE: Mutex<State> = Mutex::new(State::New);

#[derive(Deserialize)]
#[serde(
    crate = "vibe_backend::serde",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct Init {
    roaming_config: std::path::PathBuf,
    local_data: std::path::PathBuf,
    resources: std::path::PathBuf,
    version: String,
    autostart: bool,
}

/// # Safety
/// `options` is borrowed NUL-terminated UTF-8. Callback lives through close.
#[no_mangle]
pub unsafe extern "C" fn vibe_init(
    version: u32,
    options: *const c_char,
    callback: Option<Callback>,
) -> *mut c_char {
    boundary(|| {
        if version != VERSION {
            return Err(Error::Version);
        }
        let output = Arc::new(Output(callback.ok_or(Error::Arguments)?));
        let init: Init =
            serde_json::from_str(unsafe { input(options)? }).map_err(|_| Error::Arguments)?;
        if !init.roaming_config.is_absolute()
            || !init.local_data.is_absolute()
            || !init.resources.is_absolute()
            || init.version.is_empty()
        {
            return Err(Error::Arguments);
        }
        let options = Options {
            roaming_config: init.roaming_config,
            local_data: init.local_data,
            resources: init.resources,
            version: init.version,
            autostart: init.autostart,
        };
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        match *state {
            State::Open(_) => return Err(Error::AlreadyOpen),
            State::Closed(_) => return Err(Error::Closed),
            State::New => (),
        }
        let runtime = Runtime::new().map_err(|_| Error::Internal)?;
        let app = {
            let _enter = runtime.enter();
            #[cfg(any(test, feature = "smoke"))]
            let init = Backend::initialize_fixture;
            #[cfg(not(any(test, feature = "smoke")))]
            let init = Backend::initialize;
            init(options, output.clone())
        };
        let (incoming, receiver) = mpsc::unbounded_channel();
        let worker = runtime.spawn(serve(app, output, receiver));
        *state = State::Open(Session {
            runtime,
            incoming,
            worker,
            last_id: 0,
        });
        Ok(())
    })
}

#[repr(C)]
pub struct Admission {
    pub request_id: u64,
    pub error: *mut c_char,
}

/// # Safety
/// Both strings are borrowed NUL-terminated UTF-8, copied before admission.
#[no_mangle]
pub unsafe extern "C" fn vibe_call(id: u64, name: *const c_char, args: *const c_char) -> Admission {
    let error = boundary(|| {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let State::Open(session) = &mut *state else {
            return Err(Error::Closed);
        };
        if id <= session.last_id {
            return Err(Error::RequestId);
        }
        let call = Call {
            id,
            name: unsafe { input(name)? }.into(),
            args: unsafe { input(args)? }.into(),
        };
        session.incoming.send(call).map_err(|_| Error::Closed)?;
        session.last_id = id;
        Ok(())
    });
    Admission {
        request_id: if error.is_null() { id } else { 0 },
        error,
    }
}

#[no_mangle]
pub extern "C" fn vibe_close() -> *mut c_char {
    boundary(|| {
        // A callback may already be the worker another thread is joining.
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(Error::CloseFromRuntime);
        }
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        match &*state {
            State::Closed(result) => return result.clone(),
            State::New => return Ok(()),
            State::Open(_) => (),
        }
        let State::Open(session) = std::mem::replace(&mut *state, State::Closed(Err(Error::Panic)))
        else {
            unreachable!()
        };
        drop(session.incoming);
        let result = session
            .runtime
            .block_on(session.worker)
            .unwrap_or(Err(Error::Panic));
        // serve has joined the command and host-event tasks. Blocking work
        // (e.g. DNS, PAC and node -v) never calls the host directly.
        session.runtime.shutdown_timeout(Duration::from_millis(100));
        *state = State::Closed(result.clone());
        result
    })
}

/// # Safety
/// Pass null or one still-owned pointer from this DLL, exactly once.
#[no_mangle]
pub unsafe extern "C" fn vibe_free(value: *mut c_char) -> *mut c_char {
    boundary(|| {
        if !value.is_null() {
            drop(unsafe { CString::from_raw(value) });
        }
        Ok(())
    })
}
