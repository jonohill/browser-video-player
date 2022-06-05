use std::io::Error;
use tokio::process::Command;

#[derive(thiserror::Error, Debug)]
pub enum ConvertError {
    #[error("HandBrake failed: {0}")]
    HandBrakeError(String),

    #[error("IO Error: {0}")]
    IOError(#[from] Error),

    #[error("File is already being converted")]
    InProgress,
}

pub async fn convert_to_mp4(input_path: &str, output_path: &str) -> Result<(), ConvertError> {
    let tmp_output_path = output_path.to_string() + ".tmp";

    if std::path::Path::new(tmp_output_path.as_str()).exists() {
        return Err(ConvertError::InProgress);
    }

    let args = vec![
        "--input",
        input_path,
        "--output",
        &tmp_output_path,
        "--format",
        "av_mp4",
        "--encoder",
        "vt_h265",
        "--quality",
        "55",
        "--optimize",
        "--aencoder",
        "ca_aac",
        "--no-comb-detect",
        "--no-deinterlace",
        "--no-detelecine",
        "--no-nlmeans",
        "--no-chroma-smooth",
        "--no-unsharp",
        "--no-lapsharp",
        "--no-deblock",
        "--all-subtitles",
        "--all-audio",
    ];
    println!("{:?}", args.join(" "));

    let mut proc = Command::new("HandBrakeCLI")
        .args(args)
        .kill_on_drop(true)
        .spawn()?;

    let result = match proc.wait().await?.code() {
        Some(0) => Ok(()),
        Some(code) => Err(ConvertError::HandBrakeError(format!(
            "exited with code {}",
            code
        ))),
        None => Err(ConvertError::HandBrakeError("crashed".to_string())),
    };

    if result.is_ok() {
        std::fs::rename(tmp_output_path, output_path)?;
    } else {
        std::fs::remove_file(tmp_output_path)?;
    }
    result
}
