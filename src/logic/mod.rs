use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::{fs::File, io::Write};

pub mod disk;
pub mod pipeline;
pub mod security;

pub fn download(url: &str, output: &str) -> Result<()> {
    let res = reqwest::blocking::get(url)?.error_for_status()?;

    let total_size = res.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);

    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    let mut file = File::create(output)?;

    let mut downloaded: u64 = 0;
    let mut buffer = [0u8; 8192];

    let mut reader = res;

    loop {
        let bytes_read = std::io::Read::read(&mut reader, &mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])?;
        downloaded += bytes_read as u64;

        pb.set_position(downloaded);
    }

    pb.finish_with_message("download complete");

    Ok(())
}
