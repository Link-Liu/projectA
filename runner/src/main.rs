use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

/// BONUS-PROCESS: This binary demonstrates OS process management:
/// - spawn child processes (gateway + dashboard)
/// - observe exit status (try_wait)
/// - kill remaining child on failure
/// - graceful cleanup on Ctrl+C (kill + wait)
fn main() {
    let shutting_down = Arc::new(AtomicBool::new(false));

    // We'll store children in mutexes so Ctrl+C handler can kill them.
    let gateway_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let dashboard_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    {
        let shutting_down = Arc::clone(&shutting_down);
        let gateway_child = Arc::clone(&gateway_child);
        let dashboard_child = Arc::clone(&dashboard_child);

        ctrlc::set_handler(move || {
            // BONUS-PROCESS: Ctrl+C triggers cleanup of child processes.
            shutting_down.store(true, Ordering::SeqCst);

            if let Ok(mut g) = gateway_child.lock() {
                if let Some(child) = g.as_mut() {
                    let _ = child.kill();
                }
            }
            if let Ok(mut d) = dashboard_child.lock() {
                if let Some(child) = d.as_mut() {
                    let _ = child.kill();
                }
            }
        })
        .expect("failed to install Ctrl+C handler");
    }

    // BONUS-PROCESS: Spawn gateway as an OS child process.
    let gateway = spawn_child("gateway");
    eprintln!("[runner] gateway pid={}", gateway.id());
    *gateway_child.lock().unwrap() = Some(gateway);

    // BONUS-PROCESS: Spawn dashboard as an OS child process.
    let dashboard = spawn_child("dashboard");
    eprintln!("[runner] dashboard pid={}", dashboard.id());
    *dashboard_child.lock().unwrap() = Some(dashboard);

    // Supervision loop: if either exits, kill the other and exit.
    loop {
        if shutting_down.load(Ordering::SeqCst) {
            break;
        }

        let gateway_status = check_exit(&gateway_child);
        let dashboard_status = check_exit(&dashboard_child);

        match (gateway_status, dashboard_status) {
            (Some(status), _) => {
                eprintln!("[runner] gateway exited: {status}");
                kill_and_wait(&dashboard_child, "dashboard");
                std::process::exit(exit_code(&status));
            }
            (_, Some(status)) => {
                eprintln!("[runner] dashboard exited: {status}");
                kill_and_wait(&gateway_child, "gateway");
                std::process::exit(exit_code(&status));
            }
            (None, None) => {}
        }

        std::thread::sleep(Duration::from_millis(250));
    }

    // Graceful shutdown path (Ctrl+C).
    kill_and_wait(&dashboard_child, "dashboard");
    kill_and_wait(&gateway_child, "gateway");
    eprintln!("[runner] shutdown complete");
}

fn spawn_child(package: &str) -> Child {
    Command::new("cargo")
        .args(["run", "-p", package])
        // Keep output visible in the same terminal.
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("[runner] failed to spawn `{package}`: {e}"))
}

fn check_exit(child: &Arc<Mutex<Option<Child>>>) -> Option<ExitStatus> {
    let mut guard = child.lock().unwrap();
    let Some(ch) = guard.as_mut() else { return None };
    match ch.try_wait() {
        Ok(Some(status)) => {
            // Take ownership out so we don't try to manage it again.
            let _ = guard.take();
            Some(status)
        }
        Ok(None) => None,
        Err(_) => None,
    }
}

fn kill_and_wait(child: &Arc<Mutex<Option<Child>>>, name: &str) {
    let mut guard = child.lock().unwrap();
    let Some(mut ch) = guard.take() else { return };
    eprintln!("[runner] stopping {name} pid={}", ch.id());
    let _ = ch.kill();
    let _ = ch.wait();
}

fn exit_code(status: &ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        code
    } else {
        1
    }
}
