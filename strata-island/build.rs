use std::{env, fs::File, io, path::PathBuf};

fn main() -> io::Result<()> {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    for name in ["addresses-v1", "journeys-v1"] {
        let source = format!("fixtures/{name}.json.gz");
        println!("cargo:rerun-if-changed={source}");
        let mut decoder = flate2::read::GzDecoder::new(File::open(source)?);
        let mut destination = File::create(out.join(format!("{name}.json")))?;
        io::copy(&mut decoder, &mut destination)?;
    }
    Ok(())
}
