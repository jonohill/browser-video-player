use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use tempfile::NamedTempFile;

/// The RNNoise model data, embedded at compile time
const MODEL_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rnnoise_model.rnnn"));

/// Global holder for the model temp file (keeps it alive for the duration of the program)
static MODEL_FILE: OnceLock<NamedTempFile> = OnceLock::new();

/// Returns the path to a temporary file containing the RNNoise model.
/// The file is created on first call and persists for the lifetime of the program.
pub fn get_model_path() -> PathBuf {
    MODEL_FILE
        .get_or_init(|| {
            let mut file = NamedTempFile::new().expect("Failed to create temp file for RNNoise model");
            file.write_all(MODEL_DATA).expect("Failed to write RNNoise model to temp file");
            file.flush().expect("Failed to flush RNNoise model file");
            file
        })
        .path()
        .to_path_buf()
}
