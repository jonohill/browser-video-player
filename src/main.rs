use std::path::PathBuf;

use actix_files::Files;
use actix_web::{web, App, HttpResponse, HttpServer, ResponseError};
use clap::Parser;
use player::PlayerError;
use tokio::signal;

mod convert;
mod player;
mod rnnoise;
mod routes;

#[derive(Parser, Debug)]
#[command(name = "browser-video-player")]
#[command(about = "A browser-based video player server")]
struct Args {
    /// Path to a directory containing videos
    #[arg(short, long)]
    path: PathBuf,

    /// FFmpeg video codec to use for conversion
    #[arg(short, long)]
    codec: Option<String>,

    /// Number of buffers for video conversion
    #[arg(short, long, default_value_t = 5)]
    buffer_count: usize,

    /// Disable video deletion (delete requests will be ignored)
    #[arg(long, default_value_t = false)]
    no_delete: bool,

    /// Always re-encode video regardless of existing codec
    #[arg(long, default_value_t = false)]
    always_reencode: bool,

    /// Apply RNN-based noise reduction to audio (reduces background noise)
    #[arg(long, default_value_t = false)]
    denoise: bool,
}

impl ResponseError for PlayerError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError().finish()
    }

    fn status_code(&self) -> actix_web::http::StatusCode {
        actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let args = Args::parse();

    let player = web::Data::new(player::Player::new(&args.path, args.codec.as_deref(), args.buffer_count, args.no_delete, args.always_reencode, args.denoise));
    let files_dir: String = player.files_dir().to_str().unwrap().to_string();

    log::info!("Serving static files from: {}", &files_dir);

    let conversion_player = player.clone();

    let server = HttpServer::new(move || {
        App::new()
            .app_data(player.clone())
            .service(Files::new("/video-files", &files_dir))
            .service(routes::get_random)
            .service(routes::delete_video)
            .service(routes::reencode_video)
            .service(routes::get_root)
    })
    .bind(("0.0.0.0", 8081))?
    .run();

    let server_handle = server.handle();

    let conversion = async {
        let result = conversion_player.convert_all().await;
        if let Err(ref err) = result {
            log::error!("Error converting files: {}", err);
            // Stop the server when conversion fails (including interruption)
            server_handle.stop(false).await;
        }
        result.map_err(std::io::Error::other)
    };

    let ctrl_c = async {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        log::info!("Ctrl+C received, shutting down...");
        conversion_player.cancel();
        server_handle.stop(false).await;
        Ok(())
    };

    tokio::try_join!(server, conversion, ctrl_c)?;

    Ok(())
}
