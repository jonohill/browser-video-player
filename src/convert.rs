use std::io::Error;
use serde::Deserialize;
use tokio::process::Command;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

#[derive(thiserror::Error, Debug)]
pub enum ConvertError {
    #[error("HandBrake failed: {0}")]
    HandBrakeError(String),

    #[error("IO Error: {0}")]
    IOError(#[from] Error),

    #[error("File is already being converted")]
    InProgress,

    #[error("Couldn't deserialize ffprobe output")]
    FormatError(#[from] serde_json::Error),

    #[error("Interrupted by signal")]
    Interrupted,
}

pub async fn convert_to_mp4(input_path: &str, output_path: &str, codec: Option<&str>, force_reencode: bool, denoise: bool) -> Result<(), ConvertError> {
    let tmp_output_path = output_path.to_string() + ".tmp";

    if std::path::Path::new(tmp_output_path.as_str()).exists() {
        return Err(ConvertError::InProgress);
    }

    let codec = codec.unwrap_or("libx264");
    const MAX_W: u32 = 1920;
    const MAX_H: u32 = 1080;
    let vf = format!("scale=ceil(iw*min(1\\,min({}/iw\\,{}/ih))/2)*2:-2", MAX_W, MAX_H);

    // Build audio filter chain
    let model_path = crate::rnnoise::get_model_path();
    let model_path_str = model_path.to_str().unwrap();
    let af = if denoise {
        format!("arnndn=m={}:mix=0.5", model_path_str)
    } else {
        "anull".to_string()
    };

    let streams = probe_file(input_path).await?;

    if let Some(video) = streams.video() {

        #[rustfmt::skip]
        let mut args = vec![
            "-i", input_path,
            "-movflags", "faststart",
            "-af", &af,
            "-c:a", "aac",
            "-f", "mp4",
        ];
        
        let codec_name = video.codec_name.clone().unwrap_or_else(|| "".into());
        if !force_reencode && (codec_name == "h264" || codec_name == "mpeg4" || codec_name == "hevc") {
            args.extend_from_slice(&["-c:v", "copy"]);
        } else {
            args.extend_from_slice(&["-c:v", codec]);
            // VideoToolbox encoders don't support -preset, use -realtime instead
            if codec.contains("videotoolbox") {
                args.extend_from_slice(&["-realtime", "0"]);
            } else {
                args.extend_from_slice(&["-preset", "ultrafast"]);
            }
            args.extend_from_slice(&["-filter_complex", &vf]);
        }
        // Use hvc1 tag for HEVC to ensure QuickTime compatibility
        if codec.contains("hevc") || codec.contains("h265") {
            args.extend_from_slice(&["-tag:v", "hvc1"]);
        }
        args.push(tmp_output_path.as_str());
        
        println!("{:?}", args.join(" "));
        
        let mut proc = Command::new("ffmpeg")
            .args(args)
            .kill_on_drop(true)
            .spawn()?;

        let status = proc.wait().await?;
        
        #[cfg(unix)]
        {
            // Check if the process was killed by SIGINT (Ctrl+C)
            // - signal() returns Some(2) if terminated by signal
            // - ffmpeg exits with code 255 when it catches SIGINT internally
            if status.signal() == Some(2) || status.code() == Some(255) {
                // Clean up the temp file before propagating
                let _ = std::fs::remove_file(&tmp_output_path);
                return Err(ConvertError::Interrupted);
            }
        }
        
        #[cfg(not(unix))]
        {
            // On Windows, ffmpeg also exits with code 255 on Ctrl+C
            if status.code() == Some(255) {
                let _ = std::fs::remove_file(&tmp_output_path);
                return Err(ConvertError::Interrupted);
            }
        }
        
        let result = match status.code() {
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
    } else {
        Err(ConvertError::HandBrakeError("no video stream found".to_string()))
    }
}

#[derive(Deserialize, Debug)]
struct FfStream {
    codec_name: Option<String>,
    codec_type: String,
}

#[derive(Deserialize, Debug)]
struct FfFormat {
    streams: Vec<FfStream>,
}

impl FfFormat {
    fn video(&self) -> Option<&FfStream> {
        self.streams.iter().find(|s| s.codec_type == "video")
    }
}

async fn probe_file(path: &str) -> Result<FfFormat, ConvertError> {
    let args = vec!["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams", path];

    println!("{:?}", args.join(" "));

    let proc = Command::new("ffprobe")
        .args(args)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let output = proc.wait_with_output().await?;

    let format: FfFormat = serde_json::from_slice(&output.stdout)?;

    Ok(format)
}

