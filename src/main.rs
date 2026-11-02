use std::{error::Error, io, process};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Record {
    // TODO enum
    #[serde(rename = "type")]
    tx_type: String,
    client: u32,
    tx: u32,
    // TODO decimal
    amount: f64,
}

fn parse_csv() -> Result<(), Box<dyn Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(io::stdin());
    for result in rdr.deserialize() {
        let record: Record = result?;
        println!("{}", record.tx_type);
    }
    Ok(())
}


fn main() {
    if let Err(err) = parse_csv() {
        println!("error running example: {}", err);
        process::exit(1);
    }
}
