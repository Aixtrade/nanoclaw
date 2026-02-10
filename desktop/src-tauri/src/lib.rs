use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use serde::Serialize;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

struct BackendState {
    child: Option<Child>,
    ready: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackendConfig {
    base_url: String,
    auth_token: Option<String>,
}

fn project_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NANOCLAW_DIR") {
        return PathBuf::from(dir);
    }
    // From desktop/src-tauri/ go up two levels to project root
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}

fn backend_host() -> String {
    std::env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn backend_port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(3000)
}

fn backend_base_url() -> String {
    format!("http://{}:{}", backend_host(), backend_port())
}

fn backend_auth_token() -> Option<String> {
    std::env::var("NANOCLAW_API_TOKEN")
        .ok()
        .filter(|v| !v.is_empty())
}

fn is_backend_healthy(host: &str, port: u16) -> bool {
    let addr = format!("{}:{}", host, port);
    let socket = match addr.to_socket_addrs().ok().and_then(|mut a| a.next()) {
        Some(s) => s,
        None => return false,
    };

    let mut stream = match TcpStream::connect_timeout(&socket, Duration::from_millis(500)) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

    let request = format!(
        "GET /api/health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        host
    );

    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return false;
    }

    response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200")
}

fn wait_for_backend_ready(app: AppHandle, state: Arc<Mutex<BackendState>>) {
    let host = backend_host();
    let port = backend_port();

    std::thread::spawn(move || {
        for _ in 0..80 {
            let still_running = {
                let s = state.lock().unwrap();
                s.child.is_some()
            };

            if !still_running {
                return;
            }

            if is_backend_healthy(&host, port) {
                let mut should_emit = false;
                {
                    let mut s = state.lock().unwrap();
                    if s.child.is_some() && !s.ready {
                        s.ready = true;
                        should_emit = true;
                    }
                }

                if should_emit {
                    let _ = app.emit("backend-ready", ());
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                return;
            }

            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

fn mark_backend_ready(app: &AppHandle, state: &Arc<Mutex<BackendState>>) {
    {
        let mut s = state.lock().unwrap();
        s.ready = true;
    }
    let _ = app.emit("backend-ready", ());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn kill_orphan_backend_on_port(root: &PathBuf) {
    let port = backend_port();
    let lsof_output = Command::new("lsof")
        .args([
            "-nP",
            &format!("-iTCP:{}", port),
            "-sTCP:LISTEN",
            "-t",
        ])
        .output();

    let output = match lsof_output {
        Ok(v) => v,
        Err(_) => return,
    };

    let root_text = root.to_string_lossy();
    let backend_entry = root.join("dist/index.js");
    let backend_entry_text = backend_entry.to_string_lossy();

    let pids = String::from_utf8_lossy(&output.stdout);
    for line in pids.lines().filter(|v| !v.trim().is_empty()) {
        let pid = match line.trim().parse::<i32>() {
            Ok(v) if v > 0 => v,
            _ => continue,
        };

        let cmd_output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output();

        let cmd = match cmd_output {
            Ok(v) => String::from_utf8_lossy(&v.stdout).trim().to_string(),
            Err(_) => String::new(),
        };

        let is_nanoclaw_backend = cmd.contains("node")
            && cmd.contains(backend_entry_text.as_ref())
            && cmd.contains(root_text.as_ref());

        if is_nanoclaw_backend {
            let _ = signal::kill(Pid::from_raw(pid), Signal::SIGTERM);
        }
    }
}

fn spawn_backend(app: &AppHandle, state: &Arc<Mutex<BackendState>>) {
    let root = project_dir();
    let node_entry = root.join("dist/index.js");
    let host = backend_host();
    let port = backend_port();

    {
        let mut s = state.lock().unwrap();
        if let Some(child) = s.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => {
                    s.child = None;
                }
                Ok(None) => {
                    return;
                }
            }
        }
    }

    // Another NanoClaw backend is already running on configured host/port.
    // Reuse it instead of spawning a duplicate process that will fail with EADDRINUSE.
    if is_backend_healthy(&host, port) {
        eprintln!(
            "Backend already reachable at {}:{}; skipping local spawn",
            host, port
        );
        mark_backend_ready(app, state);
        return;
    }

    if !node_entry.exists() {
        eprintln!(
            "Backend not built: {} not found. Run 'npm run build' in project root first.",
            node_entry.display()
        );
        return;
    }

    let child = Command::new("node")
        .arg(&node_entry)
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child {
        Ok(mut child) => {
            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");

            {
                let mut s = state.lock().unwrap();
                s.child = Some(child);
                s.ready = false;
            }

            wait_for_backend_ready(app.clone(), Arc::clone(state));

            // Forward backend stdout and detect process exit
            let app_handle = app.clone();
            let state_clone = Arc::clone(state);
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) => eprintln!("[backend] {}", line),
                        Err(_) => break,
                    }
                }
                // Backend process ended
                {
                    let mut s = state_clone.lock().unwrap();
                    s.ready = false;
                    s.child = None;
                }
                let _ = app_handle.emit("backend-stopped", ());
            });

            // Forward stderr
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => eprintln!("[backend:err] {}", line),
                        Err(_) => break,
                    }
                }
            });
        }
        Err(e) => {
            eprintln!("Failed to spawn backend: {}", e);
        }
    }
}

fn kill_backend(state: &Arc<Mutex<BackendState>>) {
    let mut s = state.lock().unwrap();
    if let Some(ref child) = s.child {
        let pid = child.id() as i32;
        // Send SIGTERM for graceful shutdown
        let _ = signal::kill(Pid::from_raw(pid), Signal::SIGTERM);
    }
    s.ready = false;
    // Don't set child to None yet — the stdout thread will do that when the process exits

    drop(s);

    // Also stop any orphaned nanoclaw containers
    std::thread::spawn(|| {
        let root = project_dir();
        kill_orphan_backend_on_port(&root);

        let output = Command::new("docker")
            .args(["ps", "--filter", "name=nanoclaw-", "--format", "{{.Names}}"])
            .output();
        if let Ok(output) = output {
            let names = String::from_utf8_lossy(&output.stdout);
            for name in names.lines().filter(|l| !l.is_empty()) {
                let _ = Command::new("docker").args(["stop", name]).output();
            }
        }
    });
}

fn wait_for_backend_exit(state: &Arc<Mutex<BackendState>>, timeout: Duration) {
    let start = Instant::now();
    loop {
        let stopped = {
            let mut s = state.lock().unwrap();
            match s.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => {
                        s.child = None;
                        true
                    }
                    Ok(None) => false,
                    Err(_) => {
                        s.child = None;
                        true
                    }
                },
                None => true,
            }
        };

        if stopped {
            return;
        }

        if start.elapsed() >= timeout {
            let maybe_pid = {
                let s = state.lock().unwrap();
                s.child.as_ref().map(|child| child.id() as i32)
            };

            if let Some(pid) = maybe_pid {
                let _ = signal::kill(Pid::from_raw(pid), Signal::SIGKILL);
            }
            return;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

#[tauri::command]
fn get_backend_status(state: tauri::State<Arc<Mutex<BackendState>>>) -> bool {
    state.lock().unwrap().ready
}

#[tauri::command]
fn get_backend_config() -> BackendConfig {
    BackendConfig {
        base_url: backend_base_url(),
        auth_token: backend_auth_token(),
    }
}

#[tauri::command]
fn restart_backend(
    app: AppHandle,
    state: tauri::State<Arc<Mutex<BackendState>>>,
) -> Result<(), String> {
    let state = Arc::clone(&state);
    kill_backend(&state);
    wait_for_backend_exit(&state, Duration::from_secs(5));
    spawn_backend(&app, &state);
    Ok(())
}

#[tauri::command]
fn get_project_dir() -> String {
    project_dir().to_string_lossy().to_string()
}

pub fn run() {
    let backend_state = Arc::new(Mutex::new(BackendState {
        child: None,
        ready: false,
    }));

    let state_for_setup = Arc::clone(&backend_state);
    let state_for_exit = Arc::clone(&backend_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(backend_state)
        .invoke_handler(tauri::generate_handler![
            get_backend_status,
            get_backend_config,
            restart_backend,
            get_project_dir,
        ])
        .setup(move |app| {
            // Hide from Dock — app lives in menu bar only
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);

                // Accessory policy removes fullscreen capability from windows.
                // Re-enable it so the green traffic-light button offers fullscreen.
                if let Some(window) = app.get_webview_window("main") {
                    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
                    let ns_ptr = window.ns_window().unwrap();
                    let ns_win: &NSWindow = unsafe { &*(ns_ptr as *const NSWindow) };
                    let behavior = ns_win.collectionBehavior();
                    ns_win.setCollectionBehavior(
                        behavior | NSWindowCollectionBehavior::FullScreenPrimary,
                    );
                }
            }

            // Build tray menu
            let open_item =
                MenuItemBuilder::with_id("open", "Open Chat").build(app)?;
            let restart_item =
                MenuItemBuilder::with_id("restart", "Restart Backend").build(app)?;
            let quit_item =
                MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&open_item)
                .item(&restart_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let app_handle = app.handle().clone();
            let tray_state = Arc::clone(&state_for_setup);
            let tray_builder = if let Some(icon) = app.default_window_icon() {
                TrayIconBuilder::new().icon(icon.clone())
            } else {
                TrayIconBuilder::new()
            };

            tray_builder
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "restart" => {
                        let state = Arc::clone(&tray_state);
                        let app = app.clone();
                        std::thread::spawn(move || {
                            kill_backend(&state);
                            wait_for_backend_exit(&state, Duration::from_secs(5));
                            spawn_backend(&app, &state);
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Spawn backend on startup
            spawn_backend(&app_handle, &state_for_setup);

            Ok(())
        })
        .on_window_event(|window, event| {
            // Close hides instead of destroying
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                kill_backend(&state_for_exit);
            }
        });
}
