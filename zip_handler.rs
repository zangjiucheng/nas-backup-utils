use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::{FileOptions, ZipWriter};
use walkdir::WalkDir;
use zip::ZipArchive;
use log::{info};

use crate::config::COMPRESS_FILE_NAME;

pub fn compress_dir(root_dir: &Path) -> io::Result<()> {
    for entry in WalkDir::new(&root_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
    {
        compress_process(entry.path())?;
    }
    info!("Compressed all .meta files in '{}'", root_dir.display());
    Ok(())
}

pub fn extract_dir(root_dir: &Path) -> io::Result<()> {
    for entry in WalkDir::new(&root_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
    {
        extract_zip(entry.path(), true)?;
    }
    info!("Extracted all zip files in '{}'", root_dir.display());
    Ok(())
}

fn extract_zip(dir: &Path, delete_zip: bool) -> io::Result<()> {
    let zip_path = dir.join(COMPRESS_FILE_NAME);
    if !zip_path.exists() {
        return Ok(());
    }

    // Open the zip file
    let file = File::open(&zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    // Extract each file in the zip
    for i in 0..archive.len() {
        let mut zip_file = archive.by_index(i)?;
        let name = zip_file.name_raw();
        // Ensure the file has a .meta extension
        let name_str = String::from_utf8_lossy(name).to_string();
        if !name_str.ends_with(".meta") {
            info!("Skipping non-.meta file in zip: {}", name_str);
            continue;
        }
        let file_name = name_str;

        let out_path = dir.join(&file_name);
        if out_path.exists() {
            info!("File already exists, skipping: {}", out_path.display());
            continue;
        }

        let mut content = Vec::new();
        zip_file.read_to_end(&mut content)?;
        let mut out_file = File::create(&out_path)?;
        out_file.write_all(&content)?;
        info!("Extracted: {}", out_path.display());
    }

    // Optionally delete the zip file
    if delete_zip {
        fs::remove_file(&zip_path)?;
        info!("Deleted zip: {}", zip_path.display());
    }

    Ok(())
}


fn compress_process(dir: &Path) -> io::Result<()> {
    let meta_files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "meta")
                .unwrap_or(false)
        })
        .map(|entry| entry.path())
        .collect();

    if !meta_files.is_empty() {
        create_zip(dir, &meta_files)?;
        delete_meta_files(&meta_files)?;
        info!("Compressed {} .meta files into '{}'", meta_files.len(), dir.join(COMPRESS_FILE_NAME).display());
    }

    Ok(())
}

fn create_zip(dir: &Path, meta_files: &[PathBuf]) -> io::Result<()> {
    let zip_path = dir.join(COMPRESS_FILE_NAME);
    let file = fs::File::create(&zip_path)?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::<()>::default();

    for meta_file in meta_files {
        let file_name = meta_file
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid file name"))?
            .to_string_lossy();
        let content = fs::read(meta_file)?;
        zip.start_file(file_name, options)?;
        zip.write_all(&content)?;
    }

    zip.finish()?;
    Ok(())
}

fn delete_meta_files(meta_files: &[PathBuf]) -> io::Result<()> {
    for meta_file in meta_files {
        fs::remove_file(meta_file)?;
    }
    Ok(())
}
