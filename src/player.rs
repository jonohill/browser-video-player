use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use tempdir::TempDir;
use uuid::Uuid;
use walkdir::{WalkDir, DirEntry};

use crate::convert::{convert_to_mp4, ConvertError};

const VIDEO_EXTENSIONS: [&str; 10] = ["mp4", "mkv", "avi", "mpg", "wmv", "webm", "ts", "mov", "flv", "f4v"];

fn get_media_files(path: &Path) -> impl Iterator<Item = DirEntry> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.path().extension().is_some()
                && VIDEO_EXTENSIONS
                    .contains(&entry.path().extension().unwrap().to_str().unwrap())
        })
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("Convert Error: {0}")]
    ConvertError(#[from] ConvertError),

    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct File {
    pub id: String,
    pub original_path: PathBuf,
    pub path: Option<PathBuf>,
}

pub struct Player {
    media_dir: PathBuf,
    tmp_dir: TempDir,
    files: Mutex<Vec<File>>,
    codec: Option<String>,
}

impl Player {
    pub fn new(dir_path: &Path, codec: Option<&str>) -> Self {
        let mut files: Vec<_> = get_media_files(dir_path)
            .map(|entry| File {
                id: Uuid::new_v4().to_string(),
                original_path: entry.path().to_path_buf(),
                path: None,
            })
            .collect();
        files.sort_by_key(|f| f.id.clone());

        log::info!("Found {} files", files.len());

        Self {
            media_dir: dir_path.to_path_buf(),
            tmp_dir: TempDir::new("browser-player").expect("Could not create temp dir"),
            files: Mutex::new(files),
            codec: codec.map(|s| s.to_string()),
        }
    }

    pub fn files_dir(&self) -> PathBuf {
        self.tmp_dir.path().to_path_buf()
    }

    pub async fn convert_all(&self) -> Result<(), PlayerError> {
        let mut id = None;
        while let Some(mut file) = self.get_next_file(id.clone()) {
            let output = self
                .tmp_dir
                .path()
                .join(file.id.clone())
                .with_extension("mp4");
            let input = file.original_path.to_str().unwrap();
            if let Err(err) = convert_to_mp4(input, output.to_str().unwrap(), self.codec.as_deref()).await {
                log::error!("Ignoring file due to conversion error: {}", err);
                self.delete(file.id, true).await?;
                continue;
            }

            file.path = Some(output);
            let mut files = self.files.lock().unwrap();
            if let Some(original_file) = files.iter_mut().find(|f| f.id == file.id) {
                original_file.path = file.path;
            }

            id = Some(file.id.clone());
        }

        Ok(())
    }

    pub fn get_next_file(&self, after_id: Option<String>) -> Option<File> {
        let files = self.files.lock().unwrap();
        if let Some(id) = after_id {
            files
                .iter()
                .skip_while(|f| f.id != id)
                .nth(1)
                .cloned()
                .or_else(|| files.first().cloned())
        } else {
            files.first().cloned()
        }
    }

    fn get_file_base_dir(&self, file_path: &Path) -> Option<PathBuf> {
        let stripped = file_path.strip_prefix(&self.media_dir).unwrap();
        let mut parts = stripped.iter();
        let base_dir = parts.next().unwrap();
        let result = self.media_dir.join(base_dir);
        if result == file_path {
            None
        } else {
            Some(result)
        }
    }

    fn delete_empty_file_dirs(&self, file_path: &Path) {
        if let Some(base_dir) = self.get_file_base_dir(file_path) {
            let remaining_files = get_media_files(&base_dir).count();
            if remaining_files == 0 {
                log::warn!("Deleting empty file dir: {}", base_dir.display());
                std::fs::remove_dir_all(base_dir).unwrap();
            }
        }
    }

    pub async fn delete(&self, id: String, keep_original: bool) -> Result<(), PlayerError> {
        let mut files = self.files.lock().unwrap();
        let file = files.iter_mut().find(|f| f.id == id);
        if let Some(file) = file {
            log::info!("Delete: {:?}", file.original_path);
            std::fs::remove_file(file.path.as_ref().unwrap())?;
            if !keep_original {
                std::fs::remove_file(file.original_path.clone())?;
                self.delete_empty_file_dirs(&file.original_path);
            }
        }
        files.retain(|f| f.id != id);
        Ok(())
    }
}
