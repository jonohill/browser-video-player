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

    if let Some(file) = player.get_next_file(query.after_id).await? {
        Ok(HttpResponse::Ok().json(Video { id: file.id }))
    } else {
        Ok(HttpResponse::Gone().finish())
    }
}

#[delete("/video/{id}")]
async fn delete_video(
    player: web::Data<player::Player>,
    id: web::Path<String>,
) -> Result<impl Responder, PlayerError> {
    player.delete(id.into_inner()).await?;
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

    let player = web::Data::new(player::Player::new(&first_arg));
    let files_dir: String = player.files_dir().to_str().unwrap().to_string();

    log::info!("Serving static files from: {}", &files_dir);

    HttpServer::new(move || {
        App::new()
            .app_data(player.clone())
            .service(Files::new("/video-files", &files_dir))
            .service(get_random)
            .service(delete_video)
            .service(get_root)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
