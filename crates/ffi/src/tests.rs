use super::*;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::mpsc as sync,
    time::Duration,
};

fn contract() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../native/contract")
}

struct Directory(PathBuf);
impl Directory {
    fn new(label: &str) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("{label}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn options(&self) -> Options {
        Options {
            roaming_config: self.0.clone(),
            local_data: self.0.join("local"),
            resources: self.0.join("resources"),
            version: "0.8.0".into(),
            autostart: false,
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

extern "C" fn discard(value: *mut c_char) {
    assert!(unsafe { vibe_free(value) }.is_null());
}

#[test]
fn every_command_decodes_shared_input_and_encodes_the_typed_response() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut names = BTreeSet::new();
        for entry in fs::read_dir(contract().join("commands")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_str().unwrap();
            let dir = Directory::new(&format!("contract-{name}"));
            let app = Backend::initialize_fixture(dir.options(), Arc::new(Output(discard)));
            let example: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let expected = example["value"].clone();
            if name == "get_sync_history" {
                fs::write(dir.0.join("sync-history.json"), expected.to_string()).unwrap();
            }
            let actual = dispatch::dispatch(
                app.clone(),
                name,
                &example["args"].to_string(),
                Some(expected.clone()),
            )
            .await
            .unwrap();
            assert_eq!(actual, expected, "{name}");
            assert!(names.insert(name.to_owned()));
        }
        assert_eq!(
            names,
            dispatch::NAMES.iter().map(|s| (*s).to_owned()).collect()
        );
        let dir = Directory::new("invalid");
        let app = Backend::initialize_fixture(dir.options(), Arc::new(Output(discard)));
        for json in [
            "{",
            "[]",
            r#"{"force":"synthetic-secret"}"#,
            "{}",
            r#"{"force":true,"extra":1}"#,
        ] {
            let err = dispatch::run(app.clone(), "get_rate_limits", json)
                .await
                .unwrap_err();
            assert_eq!(err.json()["code"], "invalidArguments");
            assert!(!err.json().to_string().contains("synthetic-secret"));
        }
        assert!(matches!(
            dispatch::run(app.clone(), "missing", "{}").await,
            Err(Error::Unknown)
        ));
    });
}

static RESPONSES: Mutex<Option<sync::Sender<Value>>> = Mutex::new(None);
static FINISH_CALLBACK: Mutex<Option<sync::Receiver<()>>> = Mutex::new(None);
extern "C" fn capture(value: *mut c_char) {
    let value = take(value);
    let hold = value["requestId"] == 3;
    if let Some(tx) = RESPONSES.lock().unwrap().as_ref() {
        let _ = tx.send(value);
    }
    if hold {
        if let Some(finish) = FINISH_CALLBACK.lock().unwrap().take() {
            let _ = finish.recv_timeout(Duration::from_secs(5));
        }
    }
}
fn take(value: *mut c_char) -> Value {
    assert!(!value.is_null());
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
    assert!(unsafe { vibe_free(value) }.is_null());
    parsed
}

#[test]
fn synchronous_boundary_returns_an_owned_redacted_panic_error() {
    let error = boundary(|| panic!("synthetic panic"));
    assert_eq!(take(error), Error::Panic.json());
    assert!(unsafe { vibe_free(ptr::null_mut()) }.is_null());
}

fn call(id: u64, name: &str, args: &str) -> *mut c_char {
    let admission = unsafe {
        vibe_call(
            id,
            CString::new(name).unwrap().as_ptr(),
            CString::new(args).unwrap().as_ptr(),
        )
    };
    assert_eq!(
        admission.request_id,
        if admission.error.is_null() { id } else { 0 }
    );
    admission.error
}

#[test]
fn abi_completes_each_admitted_id_once_and_joins_callbacks_before_close_returns() {
    let dir = Directory::new("abi");
    let options = dir.options();
    let init = CString::new(
        json!({"roamingConfig":options.roaming_config,"localData":options.local_data,
        "resources": options.resources,"version":"0.8.0","autostart":false})
        .to_string(),
    )
    .unwrap();
    let (tx, rx) = sync::channel();
    *RESPONSES.lock().unwrap() = Some(tx);
    assert_eq!(
        take(unsafe { vibe_init(0, init.as_ptr(), Some(capture)) })["code"],
        "versionMismatch"
    );
    assert!(unsafe { vibe_init(VERSION, init.as_ptr(), Some(capture)) }.is_null());

    // Exercise the event envelope and allocation/return path with every sample.
    for entry in fs::read_dir(contract().join("events")).unwrap() {
        let event: Value =
            serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap();
        Output(capture).event(event["name"].as_str().unwrap(), event["payload"].clone());
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), event);
    }
    assert!(call(1, "__panic", "{}").is_null());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        json!({"kind":"result","requestId":1,"error":Error::Panic.json()})
    );
    assert!(call(2, "get_settings", "{").is_null());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap()["error"]["code"],
        "invalidArguments"
    );
    assert!(call(3, "__pending", "{}").is_null());
    assert_eq!(
        take(call(3, "get_sync_state", "{}"))["code"],
        "invalidRequestId"
    );
    let (release, finish) = sync::channel();
    *FINISH_CALLBACK.lock().unwrap() = Some(finish);
    let close = std::thread::spawn(|| vibe_close().is_null());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        json!({"kind":"result","requestId":3,"error":Error::Cancelled.json()})
    );
    assert!(!close.is_finished());
    release.send(()).unwrap();
    assert!(close.join().unwrap());
    assert!(vibe_close().is_null());
    assert_eq!(take(call(4, "get_settings", "{}"))["code"], "closed");
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(50)),
        Err(sync::RecvTimeoutError::Timeout)
    ));
    *RESPONSES.lock().unwrap() = None;
}
