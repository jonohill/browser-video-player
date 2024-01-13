use std::io::Error;
use serde::Deserialize;
use tokio::process::Command;

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
}

pub async fn convert_to_mp4(input_path: &str, output_path: &str, codec: Option<&str>) -> Result<(), ConvertError> {
    let tmp_output_path = output_path.to_string() + ".tmp";

    if std::path::Path::new(tmp_output_path.as_str()).exists() {
        return Err(ConvertError::InProgress);
    }

    let codec = codec.unwrap_or("libx264");
    const MAX_W: u32 = 1920;
    const MAX_H: u32 = 1080;
    const FPS: f32 = 60.;
    let vf = format!("scale=ceil(iw*min(1\\,min({}/iw\\,{}/ih))/2)*2:-1,fps={}", MAX_W, MAX_H, FPS);

    let streams = probe_file(input_path).await?;

    if let Some(video) = streams.video() {

        #[rustfmt::skip]
        let mut args = vec![
            "-i", input_path,
            "-movflags", "faststart",
            "-c:a", "aac",
            "-f", "mp4",
        ];
        
        let codec_name = video.codec_name.clone().unwrap_or_else(|| "".into());
        if video.avg_frame_rate == FPS && (codec_name == "h264" || codec_name == "mpeg4" || codec_name == "hevc") {
            args.extend_from_slice(&["-c:v", "copy"]);
        } else {
            args.extend_from_slice(&["-c:v", codec, "-preset", "ultrafast", "-filter_complex", &vf]);
        }
        args.push(tmp_output_path.as_str());
        
        println!("{:?}", args.join(" "));
        
        let mut proc = Command::new("ffmpeg")
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
    } else {
        Err(ConvertError::HandBrakeError("no video stream found".to_string()))
    }
}

fn parse_framerate<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 1 {
        return Ok(parts[0].parse::<f32>().unwrap());
    }
    if parts.len() == 2 {
        let numerator = parts[0].parse::<f32>().unwrap();
        let denominator = parts[1].parse::<f32>().unwrap();
        return Ok(numerator / denominator);
    }
    Err(serde::de::Error::custom("invalid framerate"))
}

#[derive(Deserialize, Debug)]
struct FfStream {
    codec_name: Option<String>,
    codec_type: String,
    #[serde(deserialize_with = "parse_framerate")]
    avg_frame_rate: f32,
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
