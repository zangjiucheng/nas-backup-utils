mod backup_utils;
mod config;
mod zip_handler;

use backup_utils::{traverse_backup, traverse_meta};
use chrono;
use config::{BACKUP_DIR, CHECKPOINT_NAME, REMOVE_TEMP_IMMEDIATELY, COMPRESS_FILE_NAME, SRC_DIR, TEMP_EXT};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use zip_handler::{compress_dir, extract_dir};
use std::io::Write;
use fern::colors::{Color, ColoredLevelConfig};
use log::{error, info, warn};

fn read_last_checkpoint(backup_dir: &Path) -> io::Result<PathBuf> {
    let checkpoint = backup_dir.join(CHECKPOINT_NAME);

    // Try to read the file; if it fails (e.g. not found), treat as empty
    let content = fs::read_to_string(&checkpoint)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if content.is_empty() {
        Ok(PathBuf::new())
    } else {
        Ok(backup_dir.join(content))
    }
}

fn new_checkpoint_name() -> String {
    chrono::Utc::now().format("%Y-%m-%d_%H-%M_%S").to_string()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            if src_path.file_name().map_or(false, |name| name == COMPRESS_FILE_NAME) {
                // Only copy COMPRESS_FILE_NAME
                info!("Copying {:?}", src_path);
                fs::copy(&src_path, &dst_path)?;
            }
        }
    }
    Ok(())
}

fn generate_meta(dir: &Path) -> io::Result<()> {
    info!("meta generate  = {:?}", dir);

    let _ = traverse_meta(Path::new(dir));

    // Compress the new checkpoint directory
    compress_dir(&dir)?;

    Ok(())
}

fn backup() -> io::Result<()> {
    // Read latest_checkpoint file if it exists
    let last_checkpoint = read_last_checkpoint(Path::new(BACKUP_DIR))?;

    // Generate new checkpoint name
    let new_checkpoint_name = new_checkpoint_name();
    let new_checkpoint = Path::new(BACKUP_DIR).join(&new_checkpoint_name);

    info!("src     = {:?}", SRC_DIR);
    info!("backup  = {:?}", BACKUP_DIR);
    info!("last_cp = {:?}", last_checkpoint);
    info!("new_cp  = {:?}", new_checkpoint);

    print!("Are you sure you want to create a new backup? (y/n): ");
    io::stdout().flush().unwrap();
    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm).unwrap();
    if confirm.trim().to_lowercase() != "y" {
        warn!("Backup cancelled.");
        return Ok(());
    }

    // If last_checkpoint exists, extract it to a temporary directory
    let mut extracted_checkpoint = PathBuf::new();
    if last_checkpoint.exists() && last_checkpoint.is_dir() {
        let temp_dir = Path::new(BACKUP_DIR).join(TEMP_EXT);
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)?;
        }
        fs::create_dir_all(&temp_dir)?;
        copy_dir_recursive(&last_checkpoint, &temp_dir)?;
        extract_dir(&temp_dir)?;
        extracted_checkpoint = temp_dir;
    }

    let _ = traverse_backup(Path::new(SRC_DIR), &extracted_checkpoint, &new_checkpoint);

    // Compress the new checkpoint directory
    compress_dir(&new_checkpoint)?;

    // Clean up the temporary directory
    if extracted_checkpoint.exists() && REMOVE_TEMP_IMMEDIATELY {
        fs::remove_dir_all(&extracted_checkpoint)?;
    }

    // Update the latest checkpoint file
    let latest_path = Path::new(BACKUP_DIR).join(CHECKPOINT_NAME);
    fs::write(&latest_path, new_checkpoint_name)?;
    info!("Updated latest checkpoint: {:?}", latest_path);

    Ok(())

}

fn ask_user_for_mode() -> String {
    print!("Choose mode ([b]ackup / [m]eta): ");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_lowercase()
}

fn init_logger() -> Result<(), fern::InitError> {
    let colors = ColoredLevelConfig::new()
        .info(Color::Green)
        .warn(Color::Yellow)
        .error(Color::Red);

    fern::Dispatch::new()
        .format(move |out, msg, record| {
            out.finish(format_args!(
                "{date} {level} [{target}] {msg}",
                date   = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                level  = colors.color(record.level()),
                target = record.target(),
                msg    = msg
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())          // console
        // Ensure the logs directory exists
        .chain({
            let log_dir = Path::new("logs");
            if !log_dir.exists() {
                fs::create_dir_all(log_dir)?;
            }
            fern::log_file(format!(
                "logs/process_{}.log",
                chrono::Local::now().format("%Y-%m-%d")
            ))?
        }) // file
        .apply()?;
    Ok(())
}

fn main() -> io::Result<()> {
    // Initialize logger
    if let Err(e) = init_logger() {
        eprintln!("Failed to initialize logger: {}", e);
        return Err(io::Error::new(io::ErrorKind::Other, "Logger initialization failed"));
    }
    let mode = ask_user_for_mode();
    if mode == "m" || mode == "meta" {
        // Ask user for directory to generate meta for
        info!("Enter directory to generate meta for: ");
        io::stdout().flush().unwrap();
        let mut dir_input = String::new();
        io::stdin().read_line(&mut dir_input).unwrap();
        let dir = Path::new(dir_input.trim());
        if dir.exists() && dir.is_dir() && dir.metadata().map(|m| m.permissions().readonly() == false).unwrap_or(false) {
            // Call generate_meta function
            generate_meta(dir)?;
        } else {
            error!("Invalid directory: {:?}", dir);
        }
    } else if mode == "b" || mode == "backup" {
        // Call backup function
        backup()?;
    } else {
        error!("Invalid mode selected. Exiting.");
    } 
    Ok(())
}