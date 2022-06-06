use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use tempdir::TempDir;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::convert::{convert_to_mp4, ConvertError};

const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "mkv", "avi", "mpg", "wmv"];

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
    tmp_dir: TempDir,
    files: Mutex<Vec<File>>,
}

impl Player {
    pub fn new(dir_path: &Path) -> Self {
        let mut files: Vec<_> = WalkDir::new(dir_path)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry.path().extension().is_some()
                    && VIDEO_EXTENSIONS
                        .contains(&entry.path().extension().unwrap().to_str().unwrap())
            })
            .map(|entry| File {
                id: Uuid::new_v4().to_string(),
                original_path: entry.path().to_path_buf(),
                path: None,
            })
            .collect();
        files.sort_by_key(|f| f.id.clone());

        log::info!("Found {} files", files.len());

        Self {
            tmp_dir: TempDir::new("browser-player").expect("Could not create temp dir"),
            files: Mutex::new(files),
        }
    }

    pub fn files_dir(&self) -> PathBuf {
        self.tmp_dir.path().to_path_buf()
    }

    pub async fn get_next_file(
        &self,
        after_id: Option<String>,
    ) -> Result<Option<File>, PlayerError> {
        let file = {
            let files = self.files.lock().unwrap();
            after_id
                .and_then(|id| files.iter().skip_while(|f| f.id != id).nth(1))
                .or_else(|| files.first())
                .cloned()
        };

        if let Some(mut file) = file {
            if file.path.is_none() {
                let output = self
                    .tmp_dir
                    .path()
                    .join(file.id.clone())
                    .with_extension("mp4");
                let input = file.original_path.to_str().unwrap();
                convert_to_mp4(input, output.to_str().unwrap()).await?;

                file.path = Some(output);
                let mut files = self.files.lock().unwrap();
                files.iter_mut().find(|f| f.id == file.id).unwrap().path = file.path.clone();
            }

            return Ok(Some(file));
        }

        Ok(None)
    }

    pub async fn delete(&self, id: String) -> Result<(), PlayerError> {
        let mut files = self.files.lock().unwrap();
        let file = files.iter_mut().find(|f| f.id == id);
        if let Some(file) = file {
            log::info!("Delete: {:?}", file.original_path);
            std::fs::remove_file(file.original_path.clone())?;
        }
        files.retain(|f| f.id != id);
        Ok(())
    }
}
