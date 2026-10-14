//! Child processes started by PakPos (git, stdio MCP servers).

use std::ffi::OsStr;
use std::process::Command;

/// `Command::new` that doesn't flash a console window on Windows. The app is a
/// GUI program without a console, so Windows would otherwise open one for
/// every console child (git, npx, python…).
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
