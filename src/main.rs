use std::{env, fs, io, process};

use ledger::accounts::process;

fn main() {
    env_logger::init();
    let mut args = env::args();
    args.next();
    let Some(path) = args.next() else {
        eprintln!("usage: ledger <transactions.csv>");
        process::exit(1);
    };
    let input = match fs::File::open(&path) {
        Ok(input) => input,
        Err(err) => {
            eprintln!("error running example: {err}");
            process::exit(1);
        }
    };
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if let Err(err) = process(input, &mut output) {
        eprintln!("error running example: {err}");
        process::exit(1);
    }
}
