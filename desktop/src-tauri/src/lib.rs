use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

struct BackendState {
    child: Option<Child>,
    ready: bool,
}

fn project_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NANOCLAW_DIR") {
        return PathBuf::from(dir);
    }
    // From desktop/src-tauri/ go up two levels to project root
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}

fn spawn_backend(app: &AppHandle, state: &Arc<Mutex<BackendState>>) {
    let root = project_dir();
    let node_entry = root.join("dist/index.js");

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

            // Monitor stdout for ready signal
            let app_handle = app.clone();
            let state_clone = Arc::clone(state);
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            eprintln!("[backend] {}", line);
                            if line.contains("NanoClaw running") {
                                let mut s = state_clone.lock().unwrap();
                                s.ready = true;
                                let _ = app_handle.emit("backend-ready", ());
                                // Show the window now that backend is ready
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
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

#[tauri::command]
fn get_backend_status(state: tauri::State<Arc<Mutex<BackendState>>>) -> bool {
    state.lock().unwrap().ready
}

#[tauri::command]
fn restart_backend(
    app: AppHandle,
    state: tauri::State<Arc<Mutex<BackendState>>>,
) -> Result<(), String> {
    let state = Arc::clone(&state);
    kill_backend(&state);
    // Wait a moment for the process to exit
    std::thread::sleep(std::time::Duration::from_millis(500));
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
            restart_backend,
            get_project_dir,
        ])
        .setup(move |app| {
            // Hide from Dock — app lives in menu bar only
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
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

            TrayIconBuilder::new()
                .menu(&menu)
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
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            spawn_backend(&app, &state);
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
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
