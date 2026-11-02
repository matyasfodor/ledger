use std::{error::Error, io, process};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Record {
    name: String,
}

fn parse_csv() -> Result<(), Box<dyn Error>> {
    let mut rdr = csv::Reader::from_reader(io::stdin());
    println!("before");
    for result in rdr.deserialize() {
        let record: Record = result?;
        println!("{}", record.name);
    }
    println!("after");
    Ok(())
}


fn main() {
    if let Err(err) = parse_csv() {
        println!("error running example: {}", err);
        process::exit(1);
    }
}
