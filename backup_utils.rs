use crate::config::{IGNORE_DIRS, SRC_DIR};
use chrono::Timelike;
use log::info;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::Xxh3;

#[derive(Debug)]
struct FileInfo {
    size: u64,
    hash: String,
    time_stamp: chrono::DateTime<chrono::Utc>,
}

impl PartialEq for FileInfo {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size && self.hash == other.hash
    }
}

impl FileInfo {
    fn new(size: u64, hash: String, time_stamp: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        Self {
            size,
            hash,
            time_stamp: time_stamp.unwrap_or(chrono::Utc::now().with_nanosecond(0).unwrap()),
        }
    }

    fn from_path(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        let size = metadata.len();
        let hash = compute_xxhash(path)?;
        Ok(Self::new(size, hash, None))
    }

    fn write_to_file(&self, file: &mut File) -> io::Result<()> {
        writeln!(
            file,
            "{}\n{}\n{}",
            self.size,
            self.hash,
            self.time_stamp.timestamp()
        )?;
        Ok(())
    }

    fn read_from_meta(file: &mut File) -> io::Result<Self> {
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let mut lines = contents.lines();

        let size = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing size"))?
            .parse::<u64>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid size"))?;
        let hash = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing hash"))?
            .to_string();
        let time_stamp = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing timestamp"))
            .and_then(|ts| {
                ts.parse::<i64>()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid timestamp"))
                    .and_then(|ts| {
                        chrono::DateTime::from_timestamp(ts, 0)
                            .ok_or_else(|| {
                                io::Error::new(io::ErrorKind::InvalidData, "Invalid timestamp")
                            })
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                    })
            })?;

        Ok(Self {
            size,
            hash,
            time_stamp,
        })
    }
}

fn compute_xxhash(file_path: &Path) -> io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut hasher = Xxh3::new();
    let mut buffer = [0u8; 4096];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hasher.digest();
    Ok(format!("{:016x}", hash))
}

fn dealing_with_file(
    path: &Path,
    last_checkpoint_meta: &Option<PathBuf>,
    new_checkpoint_dir: &Path,
) -> io::Result<()> {
    // Check if the file exists in the last checkpoint
    let last_file_info = if let Some(last_checkpoint_meta) = last_checkpoint_meta {
        if last_checkpoint_meta.exists() {
            let mut file = File::open(last_checkpoint_meta)?;
            Some(FileInfo::read_from_meta(&mut file)?)
        } else {
            None
        }
    } else {
        None
    };
    let current_file_info = FileInfo::from_path(path)?;
    let new_meta_file = new_checkpoint_dir.with_extension("meta");

    // Create the new checkpoint directory if it doesn't exist
    if let Some(parent) = new_checkpoint_dir.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut meta_file_handle = File::create(&new_meta_file)?;
    current_file_info.write_to_file(&mut meta_file_handle)?;

    // Check if the file exists in the last checkpoint and if it has changed
    // If the file exists in the last checkpoint and hasn't changed, skip copying only creating the meta file
    if let Some(last_info) = &last_file_info {
        if last_info.eq(&current_file_info) {
            // No changes, skip copying
            info!("No changes for {:?}", path);
            return Ok(());
        }
    }

    // If the file doesn't exist in the last checkpoint or has changed, copy it
    // Copy the file to the new checkpoint directory
    info!("Copied {:?} -> {:?}", path, new_checkpoint_dir);
    fs::copy(path, new_checkpoint_dir)?;

    Ok(())
}

pub fn traverse_backup(
    dir: &Path,
    last_checkpoint: &Path,
    new_checkpoint: &Path,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        let rel = path
            .strip_prefix(SRC_DIR)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let dest = new_checkpoint.join(rel);

        if ft.is_dir() {
            if IGNORE_DIRS.iter().any(|ignore| path.starts_with(ignore)) {
                info!("Ignoring directory {:?}", path);
                continue;
            }
            // ensure the folder exists, then recurse
            fs::create_dir_all(&dest)?;
            traverse_backup(&path, last_checkpoint, new_checkpoint)?;
        } else if ft.is_file() {
            // ensure parent dirs exist, then copy
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let last_checkpoint_meta = if !last_checkpoint.as_os_str().is_empty() {
                Some(last_checkpoint.join(rel).with_extension("meta"))
            } else {
                None
            };
            dealing_with_file(&path, &last_checkpoint_meta, &dest)?;
        }
    }
    Ok(())
}

pub fn traverse_meta(checkpoint: &Path) -> io::Result<()> {
    for entry in fs::read_dir(checkpoint)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            if IGNORE_DIRS.iter().any(|ignore| path.starts_with(ignore)) {
                info!("Ignoring directory {:?}", path);
                continue;
            }
            traverse_meta(&path)?;
        } else if ft.is_file() {
            if path.extension().and_then(|ext| ext.to_str()) == Some("meta") {
                info!("Skipping meta file {:?}", path);
                continue;
            }
            let current_file_info = FileInfo::from_path(&path)?;
            let new_meta_file = path.with_extension("meta");

            let mut meta_file_handle = File::create(&new_meta_file)?;
            current_file_info.write_to_file(&mut meta_file_handle)?;
            info!("Created meta file for {:?}", path);
        }
    }
    Ok(())
}
