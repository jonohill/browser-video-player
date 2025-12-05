use std::env;
use std::fs;
use std::path::Path;

const MODEL_URL: &str = "https://raw.githubusercontent.com/GregorR/rnnoise-models/master/somnolent-hogwash-2018-09-01/sh.rnnn";
const MODEL_FILENAME: &str = "rnnoise_model.rnnn";

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let model_path = Path::new(&out_dir).join(MODEL_FILENAME);

    // Only download if not already present
    if !model_path.exists() {
        println!("cargo:warning=Downloading RNNoise model...");

        let response = ureq::get(MODEL_URL)
            .call()
            .expect("Failed to download RNNoise model");

        let mut file = fs::File::create(&model_path).expect("Failed to create model file");
        let mut body = response.into_body();
        std::io::copy(&mut body.as_reader(), &mut file).expect("Failed to write model file");

        println!("cargo:warning=RNNoise model downloaded successfully");
    }

    // Tell Cargo to rerun if model is missing
    println!("cargo:rerun-if-changed=build.rs");
}
