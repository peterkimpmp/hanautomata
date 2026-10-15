//! hanautomata-ime-setup: registers, checks and removes the Hanautomata TSF input method for this user (W-02), one step
//! at a time, and reports each step's result for the S0 record.

#[cfg(windows)]
fn main() {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|arg| arg == "--learning-companion") {
        std::process::exit(hanautomata_tsf::learning_runtime::companion_main(&arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "--learning-manage") {
        if arguments.len() != 1 { eprintln!("--learning-manage takes no arguments"); std::process::exit(2); }
        std::process::exit(hanautomata_tsf::learning_pipe::management_main());
    }
    if arguments.first().is_some_and(|arg| arg == "learning-prepare-rollback") {
        if arguments.len() != 1 { eprintln!("learning-prepare-rollback takes no arguments"); std::process::exit(2); }
        let result = hanautomata_tsf::system::settings_dir().map_err(|_| hanautomata_core::learning::Error::Io)
            .and_then(|directory| hanautomata_tsf::learning_pipe::prepare_rollback(&directory.join("tsf-learning.db")));
        match result {
            Ok(retired) => { println!("Learning store is V1-compatible; retired {retired} dead managed leases. Keep TSF hosts closed until rollback finishes."); std::process::exit(0); },
            Err(error) => {
                eprintln!("Learning rollback not ready: {error}. No apps were closed and no registration was changed.");
                if error == hanautomata_core::learning::Error::InvalidRequest {
                    eprintln!("Use a normal, non-elevated desktop PowerShell window.");
                }
                std::process::exit(2);
            },
        }
    }
    let Ok(arguments) = arguments.into_iter().map(|arg| arg.into_string()).collect::<Result<Vec<_>, _>>() else {
        eprintln!("Setup command must be valid Unicode"); std::process::exit(2);
    };
    std::process::exit(hanautomata_tsf::registration::cli(arguments));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hanautomata-ime-setup runs on Windows only");
    std::process::exit(2);
}
