//! hanautomata-ime-setup: registers, checks and removes the Hanautomata TSF input method for this user (W-02), one step
//! at a time, and reports each step's result for the S0 record.

#[cfg(windows)]
fn main() {
    std::process::exit(hanautomata_tsf::registration::cli(std::env::args().skip(1).collect()));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hanautomata-ime-setup runs on Windows only");
    std::process::exit(2);
}
