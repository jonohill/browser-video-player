use std::{env, path::PathBuf};

use actix_files::Files;
use actix_web::{delete, get, web, App, HttpResponse, HttpServer, Responder, ResponseError};
use player::PlayerError;
use serde::{Deserialize, Serialize};

mod convert;
mod player;

impl ResponseError for PlayerError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError().finish()
    }

    fn status_code(&self) -> actix_web::http::StatusCode {
        actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}

#[derive(Serialize)]
struct Video {
    id: String,
}

#[derive(Deserialize)]
struct NextQuery {
    after_id: Option<String>,
}

#[get("/video/next")]
async fn get_random(
    player: web::Data<player::Player>,
    query: web::Query<NextQuery>,
) -> Result<impl Responder, PlayerError> {
    let query = query.into_inner();

    if let Some(file) = player.get_next_file(query.after_id) {
        if file.path.is_some() {
            Ok(HttpResponse::Ok().json(Video { id: file.id }))
        } else {
            Ok(HttpResponse::ServiceUnavailable().finish())
        }
    } else {
        Ok(HttpResponse::Gone().finish())
    }
}

#[derive(Deserialize)]
struct DeleteQuery {
    keep: Option<bool>,
}

#[delete("/video/{id}")]
async fn delete_video(
    player: web::Data<player::Player>,
    id: web::Path<String>,
    query: web::Query<DeleteQuery>,
) -> Result<impl Responder, PlayerError> {
    let keep = query.into_inner().keep.unwrap_or(false);
    player.delete(id.into_inner(), keep).await?;
    Ok(HttpResponse::NoContent().finish())
}

#[get("/")]
async fn get_root() -> Result<impl Responder, PlayerError> {
    let page: &'static [u8] = include_bytes!("index.html");
    Ok(HttpResponse::Ok().content_type("text/html").body(page))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let first_arg: PathBuf = env::args()
        .nth(1)
        .expect("Please provide a path to a directory containing videos")
        .into();

    let codec = env::args().nth(2);

    let player = web::Data::new(player::Player::new(&first_arg, codec.as_deref()));
    let files_dir: String = player.files_dir().to_str().unwrap().to_string();

    log::info!("Serving static files from: {}", &files_dir);

    let conversion_player = player.clone();
    let conversion = async {
        conversion_player.convert_all().await.map_err(|err| {
            log::error!("Error converting files: {}", err);
            std::io::Error::new(std::io::ErrorKind::Other, err)
        })
    };

    let server = HttpServer::new(move || {
        App::new()
            .app_data(player.clone())
            .service(Files::new("/video-files", &files_dir))
            .service(get_random)
            .service(delete_video)
            .service(get_root)
    })
    .bind(("0.0.0.0", 8081))?
    .run();

    tokio::try_join!(server, conversion)?;

    Ok(())
}
