use std::path::PathBuf;

use asset_transport::{ensure_artifacts_dir, games_root_from_env};

#[cfg(windows)]
mod first_run;

#[global_allocator]
static PROCESS_ALLOCATOR: diag::ProcessCountingAllocator = diag::ProcessCountingAllocator;

fn main() {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let skycraft = take_skycraft_flags(&mut args);
    bootstrap::bench::arm();
    prepare_process_root().unwrap_or_else(|e| {
        diag::exit_launch_error(&e);
    });
    #[cfg(windows)]
    if !skycraft {
        // Also for a shortcut that names a map, so it works on first launch.
        first_run::prepare().unwrap_or_else(|e| first_run::fail(&e));
        if args.is_empty() {
            args.push("menu".into());
        }
    }
    let artifacts = ensure_artifacts_dir().unwrap_or_else(|e| diag::exit_launch_error(&e));
    announce_log(diag::init_log(&artifacts));
    let (mode, acceptance) =
        bootstrap::parse_cli(args.into_iter()).unwrap_or_else(|e| diag::exit_launch_error(&e));
    let games = games_root_from_env().unwrap_or_else(|e| diag::exit_launch_error(&e));
    bootstrap::launch(games, artifacts, mode, acceptance);
}

/// `--skycraft` and `--games <dir>`: how SkyCraft's Skyrim plugin starts a
/// guest. It starts it through Explorer's shell (outside Mod Organizer's
/// virtual file system), which passes no environment of its own, so these
/// stand in for `IW4L_SKYCRAFT=1` and `IW4L_GAMES`. A guest skips the
/// first-run setup: SkyCraft has already said where MW2 is, and nobody is
/// there to answer its dialogs.
fn take_skycraft_flags(args: &mut Vec<String>) -> bool {
    let mut skycraft = false;
    let mut games = None;
    let mut rest = Vec::with_capacity(args.len());
    let mut iter = std::mem::take(args).into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--skycraft" => skycraft = true,
            "--games" => games = iter.next(),
            _ => rest.push(arg),
        }
    }
    *args = rest;
    // SAFETY: the very start of `main`, before anything else runs or reads the
    // environment, so no other thread can be touching it.
    unsafe {
        if skycraft {
            std::env::set_var(frame::skycraft::ENV, "1");
        }
        if let Some(games) = games {
            std::env::set_var("IW4L_GAMES", games);
        }
    }
    skycraft || frame::skycraft::active()
}

fn prepare_process_root() -> Result<(), String> {
    #[cfg(windows)]
    {
        let exe =
            std::env::current_exe().map_err(|error| format!("cannot locate iw4l.exe: {error}"))?;
        let root = exe
            .parent()
            .ok_or_else(|| format!("iw4l.exe has no parent directory: {}", exe.display()))?;
        std::env::set_current_dir(root).map_err(|error| {
            format!(
                "cannot enter launcher directory {}: {error}",
                root.display()
            )
        })?;
    }
    Ok(())
}

fn announce_log(path: PathBuf) {
    diag::announce_log_stdout(&path, diag::latest_log_path().as_deref());
    diag::info!(Launch, "log: {}", path.display());
}
