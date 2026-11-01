//! The commands the TV runs Lounge's executable for as root, instead of a window: Home
//! (`--home`, from `tv/home.sh`), an installed app's icon (`--read-app-icon`) and removing an
//! app (`--uninstall-app`, both through Homebrew Channel's exec). They used to be scripts on
//! the TV's Node.js, which spends 4-5 s of CPU just starting, beside the apps on screen.
//! Each prints one Luna-style JSON reply on stdout.

/// Runs the command `args` (the arguments after the program) name. None: not a helper
/// command, start Lounge. Some(exit code) once it ran.
pub fn run(args: &[String]) -> Option<i32> {
    let flag = args.first()?.as_str();
    if !["--home", "--read-app-icon", "--uninstall-app"].contains(&flag) {
        return None;
    }
    #[cfg(all(feature = "webos", unix))]
    let (reply, ok) = tv(
        flag,
        args.get(1).map(String::as_str),
        args.get(2).map(String::as_str),
        args.get(3).map(String::as_str),
    );
    // A desktop build never mounts, kills or removes anything.
    #[cfg(not(all(feature = "webos", unix)))]
    let (reply, ok) = (
        crate::home_mount::failure("TV helper commands need a webOS build"),
        false,
    );
    println!("{reply}");
    Some(if ok { 0 } else { 1 })
}

#[cfg(all(feature = "webos", unix))]
fn tv(
    flag: &str,
    argument: Option<&str>,
    app_dir: Option<&str>,
    target: Option<&str>,
) -> (String, bool) {
    let missing = || (crate::home_mount::failure("Missing argument"), false);
    match flag {
        "--home" => {
            let Some(command) = argument else {
                return missing();
            };
            // SAFETY: geteuid has no preconditions.
            if unsafe { libc::geteuid() } != 0 {
                return (crate::home_mount::failure("Run as root"), false);
            }
            let home = crate::home_mount::Home::new(crate::home_mount::Tv);
            crate::home_mount::command(&home, command, app_dir, target)
        }
        "--read-app-icon" => {
            let Some(source) = argument else {
                return missing();
            };
            match crate::app_artwork::installed_icon(source, crate::app_artwork::read_file) {
                Ok(reply) => (reply.to_string(), true),
                Err(error) => (crate::home_mount::failure(&error), false),
            }
        }
        // Always a reply the context menu reads, as the removal succeeded or not.
        _ => (
            crate::uninstall::remove(argument.unwrap_or_default()).to_string(),
            true,
        ),
    }
}
