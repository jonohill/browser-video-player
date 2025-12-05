use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use walkdir::{WalkDir, DirEntry};

use crate::convert::{convert_to_mp4, ConvertError};

const VIDEO_EXTENSIONS: [&str; 11] = ["mp4", "mkv", "avi", "mpg", "wmv", "webm", "ts", "mov", "flv", "f4v", "m4v"];

fn get_media_files(path: &Path) -> impl Iterator<Item = DirEntry> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.path().extension().is_some()
                && VIDEO_EXTENSIONS
                    .contains(&entry.path().extension().unwrap().to_ascii_lowercase().to_str().unwrap())
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
    buffer_count: usize,
    no_delete: bool,
    always_reencode: bool,
    denoise: bool,
    delete_notify_tx: mpsc::Sender<()>,
    delete_notify_rx: Mutex<Option<mpsc::Receiver<()>>>,
    cancellation_token: CancellationToken,
}

impl Player {
    pub fn new(dir_path: &Path, codec: Option<&str>, buffer_count: usize, no_delete: bool, always_reencode: bool, denoise: bool) -> Self {
        let mut files: Vec<_> = get_media_files(dir_path)
            .map(|entry| File {
                id: Uuid::new_v4().to_string(),
                original_path: entry.path().to_path_buf(),
                path: None,
            })
            .collect();
        files.sort_by_key(|f| f.id.clone());

        log::info!("Found {} files", files.len());

        let (tx, rx) = mpsc::channel(16);

        Self {
            media_dir: dir_path.to_path_buf(),
            tmp_dir: tempfile::Builder::new().prefix("browser-player").tempdir().expect("Could not create temp dir"),
            files: Mutex::new(files),
            codec: codec.map(|s| s.to_string()),
            buffer_count,
            no_delete,
            always_reencode,
            denoise,
            delete_notify_tx: tx,
            delete_notify_rx: Mutex::new(Some(rx)),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn files_dir(&self) -> PathBuf {
        self.tmp_dir.path().to_path_buf()
    }

    /// Cancels the conversion loop, allowing it to exit gracefully
    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    /// Returns the number of converted files currently in the queue
    fn converted_count(&self) -> usize {
        let files = self.files.lock().unwrap();
        files.iter().filter(|f| f.path.is_some()).count()
    }

    /// Returns the next file that needs to be converted (has no path yet)
    fn get_next_unconverted(&self) -> Option<File> {
        let files = self.files.lock().unwrap();
        files.iter().find(|f| f.path.is_none()).cloned()
    }

    pub async fn convert_all(&self) -> Result<(), PlayerError> {
        // Take ownership of the receiver
        let mut rx = self.delete_notify_rx.lock().unwrap().take()
            .expect("convert_all can only be called once");

        loop {
            // Convert files until we reach the buffer count
            while self.converted_count() < self.buffer_count {
                if let Some(mut file) = self.get_next_unconverted() {
                    let output = self
                        .tmp_dir
                        .path()
                        .join(file.id.clone())
                        .with_extension("mp4");
                    let input = file.original_path.to_str().unwrap();
                    
                    match convert_to_mp4(input, output.to_str().unwrap(), self.codec.as_deref(), self.always_reencode, self.denoise).await {
                        Err(ConvertError::Interrupted) => {
                            // Ctrl+C was pressed, propagate by returning an error
                            log::info!("Conversion interrupted by signal");
                            return Err(PlayerError::ConvertError(ConvertError::Interrupted));
                        }
                        Err(err) => {
                            log::error!("Ignoring file due to conversion error: {}", err);
                            self.delete(file.id.clone(), true).await?;
                            continue;
                        }
                        Ok(()) => {}
                    }

                    file.path = Some(output);
                    let mut files = self.files.lock().unwrap();
                    if let Some(original_file) = files.iter_mut().find(|f| f.id == file.id) {
                        original_file.path = file.path;
                    }
                } else {
                    // No more files to convert
                    break;
                }
            }

            // Check if there are any files left at all
            if self.files.lock().unwrap().is_empty() {
                log::info!("All files processed, conversion complete");
                return Ok(());
            }

            // Wait for a delete notification before checking again
            log::info!("Buffer full ({} converted), waiting for delete...", self.converted_count());
            tokio::select! {
                _ = self.cancellation_token.cancelled() => {
                    log::info!("Conversion cancelled");
                    return Ok(());
                }
                result = rx.recv() => {
                    if result.is_none() {
                        // Channel closed, we're done
                        return Ok(());
                    }
                }
            }
        }
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
        if self.no_delete {
            let files = self.files.lock().unwrap();
            if let Some(file) = files.iter().find(|f| f.id == id) {
                log::info!("Delete requested but ignored (--no-delete): {:?}", file.original_path);
            }
            return Ok(());
        }

        let original_path;
        {
            let mut files = self.files.lock().unwrap();
            let file = files.iter_mut().find(|f| f.id == id);
            if let Some(file) = file {
                log::info!("Delete: {:?}", file.original_path);
                if let Some(ref path) = file.path {
                    std::fs::remove_file(path)?;
                }
                original_path = Some(file.original_path.clone());
                if !keep_original {
                    std::fs::remove_file(file.original_path.clone())?;
                }
            } else {
                original_path = None;
            }
            files.retain(|f| f.id != id);
        }
        
        if let Some(path) = original_path {
            if !keep_original {
                self.delete_empty_file_dirs(&path);
            }
            // Notify the converter that a file was deleted
            let _ = self.delete_notify_tx.send(()).await;
        }
        Ok(())
    }

    pub fn remove_from_queue(&self, id: &str) -> Option<File> {
        let mut files = self.files.lock().unwrap();
        let index = files.iter().position(|f| f.id == id)?;
        Some(files.remove(index))
    }

    pub async fn reencode_file(&self, file: File) -> Result<(), PlayerError> {
        log::info!("Re-encoding: {:?}", file.original_path);

        // Delete the existing converted file if it exists
        if let Some(ref path) = file.path && path.exists() {
            std::fs::remove_file(path)?;
        }

        // Re-encode the video with forced video transcoding
        let output = self
            .tmp_dir
            .path()
            .join(file.id.clone())
            .with_extension("mp4");
        let input = file.original_path.to_str().unwrap();
        
        convert_to_mp4(input, output.to_str().unwrap(), self.codec.as_deref(), true, self.denoise).await?;

        // Add the file back to the end of the queue
        let mut new_file = file.clone();
        new_file.path = Some(output);
        
        let mut files = self.files.lock().unwrap();
        files.push(new_file);

        Ok(())
    }
}
