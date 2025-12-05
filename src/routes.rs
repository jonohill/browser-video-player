use actix_web::{delete, get, post, web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::player::{Player, PlayerError};

#[derive(Serialize)]
struct Video {
    id: String,
}

#[derive(Deserialize)]
struct NextQuery {
    after_id: Option<String>,
}

#[get("/video/next")]
pub async fn get_random(
    player: web::Data<Player>,
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
pub async fn delete_video(
    player: web::Data<Player>,
    id: web::Path<String>,
    query: web::Query<DeleteQuery>,
) -> Result<impl Responder, PlayerError> {
    let keep = query.into_inner().keep.unwrap_or(false);
    player.delete(id.into_inner(), keep).await?;
    Ok(HttpResponse::NoContent().finish())
}

#[post("/video/{id}/reencode")]
pub async fn reencode_video(
    player: web::Data<Player>,
    id: web::Path<String>,
) -> Result<impl Responder, PlayerError> {
    let id_str = id.into_inner();
    
    // Remove from queue immediately (synchronous operation)
    if let Some(file) = player.remove_from_queue(&id_str) {
        let player_clone = player.clone();
        
        // Spawn the re-encoding task in the background after removal
        tokio::spawn(async move {
            if let Err(err) = player_clone.reencode_file(file).await {
                log::error!("Error re-encoding video {}: {}", id_str, err);
            }
        });
    }
    
    Ok(HttpResponse::Accepted().finish())
}

#[get("/")]
pub async fn get_root() -> Result<impl Responder, PlayerError> {
    let page: &'static [u8] = include_bytes!("index.html");
    Ok(HttpResponse::Ok().content_type("text/html").body(page))
}
