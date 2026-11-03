use std::{env, fs, io, process};

use ledger::process;

fn main() {
    env_logger::init();
    let mut args = env::args();
    args.next();
    let Some(path) = args.next() else {
        log::error!("usage: ledger <transactions.csv>");
        process::exit(1);
    };
    let input = match fs::File::open(&path) {
        Ok(input) => input,
        Err(err) => {
            log::error!("failed to open {path}: {err}");
            process::exit(1);
        }
    };
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if let Err(err) = process(input, &mut output) {
        log::error!("failed to write the report: {err}");
        process::exit(1);
    }
}
