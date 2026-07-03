//! Windows process helpers (adapted from ATM's process_utils.rs):
//! spawn children without console-window flashes; kill whole process trees.

use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Hide the console window a spawned command would otherwise flash.
pub fn hide_command_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

pub fn hide_tokio_command_window(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Kill a child and its whole descendant tree.
/// Windows `Child::kill` only terminates the direct child (e.g. node.exe would
/// survive a killed cmd.exe) — use `taskkill /T /F` instead.
pub fn kill_child_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let mut kill = Command::new("taskkill");
            kill.args(["/PID", &pid.to_string(), "/T", "/F"]);
            hide_command_window(&mut kill);
            let _ = kill.status();
            return;
        }
    }
    let _ = child.start_kill();
}
