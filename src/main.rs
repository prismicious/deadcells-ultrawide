//! Dead Cells ultrawide launcher: start the game and widen its view so it fills the screen.
//!
//! Dead Cells (HashLink bytecode inside deadcells.exe) sizes its play area in
//! Main.onResize as
//!
//!     pixelScale   = floor4(max(1, 3 * min(W / 1920, H / 1080)))   // 0.25 steps
//!     stage width  = min(pixelScale * Viewport.NATIVE_WIDTH,  W)
//!     stage height = min(pixelScale * Viewport.NATIVE_HEIGHT, H)
//!
//! and paints everything outside the stage black. NATIVE_WIDTH is 683, about 16:9,
//! so a 5120x1440 screen gets a 2732 px wide stage and 1194 px of black on each side.
//! The HUD, menus and parallax backgrounds all lay out against the stage size and
//! NATIVE_WIDTH. Raising that single static therefore widens the world view (Hor+)
//! with everything following it, and nothing is stretched.
//!
//! The exe is protected by SteamStub DRM, so editing it on disk fails with Steam's
//! "Application load error 3:0000065432". This launcher leaves every game file
//! untouched. Instead it:
//!
//!  1. starts the game through Steam, or runs the command Steam hands it when used as
//!     a Steam launch-option wrapper (`"...\deadcells-ultrawide.exe" %command%`);
//!  2. waits for the game to initialise the $Viewport statics, which show up in memory
//!     as the aligned int32 pair 683, 400 (NATIVE_WIDTH, NATIVE_HEIGHT);
//!  3. writes the new NATIVE_WIDTH into that one location, reads it back, and resizes
//!     the game window by a pixel and back so Main.onResize runs again.

#![windows_subsystem = "windows"]

mod game;
mod output;
mod win32;

use output::say;
use std::process::{Child, Command, ExitCode};

const STEAM_APP_ID: u32 = 588650;

const USAGE: &str = "\
Dead Cells ultrawide launcher: start the game and widen its view so it fills the screen.

usage: deadcells-ultrawide.exe [--width WIDTH] [--attach] [command ...]

  (no arguments)   launch Dead Cells via Steam and fit the primary monitor
  --width WIDTH    logical NATIVE_WIDTH to use instead of fitting the monitor
  --attach         patch an already running game instead of launching it
  command ...      game command from Steam's %command% (Steam launch options)
  -h, --help       show this help";

struct Args {
    width: Option<i32>,
    attach: bool,
    command: Vec<String>,
}

enum Parsed {
    Run(Args),
    Help,
}

fn parse_args(mut rest: impl Iterator<Item = String>) -> Result<Parsed, String> {
    let mut args = Args {
        width: None,
        attach: false,
        command: Vec::new(),
    };
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--attach" => args.attach = true,
            "--width" => {
                let value = rest.next().ok_or("--width needs a number")?;
                args.width = Some(parse_width(&value)?);
            }
            _ if arg.starts_with("--width=") => {
                args.width = Some(parse_width(&arg["--width=".len()..])?)
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}")),
            _ => {
                // Everything from the first non-option on is the game command.
                args.command.push(arg);
                args.command.extend(rest);
                break;
            }
        }
    }
    Ok(Parsed::Run(args))
}

fn parse_width(value: &str) -> Result<i32, String> {
    value
        .parse()
        .map_err(|_| format!("--width: {value:?} is not a whole number"))
}

/// Primary monitor size and the logical width that fills it at the game's own pixelScale.
fn logical_width_for_primary_monitor() -> (i32, i32, i32) {
    let w = unsafe { win32::GetSystemMetrics(win32::SM_CXSCREEN) };
    let h = unsafe { win32::GetSystemMetrics(win32::SM_CYSCREEN) };
    let scale = ((3.0 * (w as f64 / 1920.0).min(h as f64 / 1080.0)).max(1.0) * 4.0).floor() / 4.0;
    (w, h, (w as f64 / scale).ceil() as i32)
}

/// Start the game: the wrapped Steam command if given, else through Steam itself.
fn launch(command: &[String]) -> Result<Option<Child>, String> {
    if let [program, args @ ..] = command {
        return Command::new(program)
            .args(args)
            .spawn()
            .map(Some)
            .map_err(|e| format!("cannot start {program}: {e}"));
    }
    let url = win32::wide(&format!("steam://rungameid/{STEAM_APP_ID}"));
    let open = win32::wide("open");
    let result = unsafe {
        win32::ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            win32::SW_SHOWNORMAL,
        )
    };
    if result <= 32 {
        return Err(format!(
            "cannot start Steam (ShellExecute returned {result})"
        ));
    }
    Ok(None)
}

fn run(args: Args) -> Result<ExitCode, String> {
    unsafe {
        win32::SetProcessDpiAwarenessContext(win32::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
    };
    let mut width = args.width;
    if width.is_none() {
        let (w, h, fit) = logical_width_for_primary_monitor();
        say(&format!("primary monitor {w}x{h} -> logical width {fit}"));
        width = Some(fit);
    }
    let width = width.filter(|&w| w > game::STOCK_WIDTH);
    if width.is_none() {
        say("screen is 16:9 or narrower; the stock game already fills it");
    }

    if !args.attach && game::find_pid().is_some() {
        return Err("Dead Cells is already running; close it or use --attach".into());
    }
    let child = if args.attach {
        None
    } else {
        launch(&args.command)?
    };
    let patched = width.map_or(Ok(()), game::patch);
    let Some(mut child) = child else {
        return patched.map(|()| ExitCode::SUCCESS);
    };
    // Steam tracks the wrapper, so stay alive while the game runs, patched or not.
    if let Err(e) = &patched {
        say(&format!("not patched, the game runs stock: {e}"));
    }
    let status = child
        .wait()
        .map_err(|e| format!("waiting for the game failed: {e}"))?;
    Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

fn main() -> ExitCode {
    output::init();
    let outcome = match parse_args(std::env::args().skip(1)) {
        Ok(Parsed::Help) => {
            say(USAGE);
            Ok(ExitCode::SUCCESS)
        }
        Ok(Parsed::Run(args)) => run(args),
        Err(e) => Err(format!("{e}\n\n{USAGE}")),
    };
    outcome.unwrap_or_else(|e| {
        say(&format!("error: {e}"));
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Parsed, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    fn run_args(args: &[&str]) -> Args {
        match parse(args) {
            Ok(Parsed::Run(a)) => a,
            _ => panic!("expected run args for {args:?}"),
        }
    }

    #[test]
    fn no_arguments_launches_via_steam_and_fits_the_monitor() {
        let a = run_args(&[]);
        assert_eq!((a.width, a.attach, a.command.len()), (None, false, 0));
    }

    #[test]
    fn options_then_steam_command() {
        let a = run_args(&[
            "--width",
            "1280",
            r"C:\Games\Dead Cells\deadcells.exe",
            "--attach",
        ]);
        assert_eq!(a.width, Some(1280));
        assert!(!a.attach, "options after the command belong to the game");
        assert_eq!(
            a.command,
            [r"C:\Games\Dead Cells\deadcells.exe", "--attach"]
        );
    }

    #[test]
    fn width_with_equals_and_attach() {
        let a = run_args(&["--width=900", "--attach"]);
        assert_eq!((a.width, a.attach), (Some(900), true));
    }

    #[test]
    fn bad_input_is_rejected() {
        assert!(parse(&["--width", "wide"]).is_err());
        assert!(parse(&["--width"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
        assert!(matches!(parse(&["--help"]), Ok(Parsed::Help)));
    }
}
