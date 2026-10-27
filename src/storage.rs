use std::fs;
use std::io::{self, Cursor};
use std::path::PathBuf;
use windows::Win32::System::SystemInformation::GetLocalTime;

pub fn get_save_dir() -> io::Result<PathBuf> {
    let user_profile = std::env::var("USERPROFILE")
        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
    let path = PathBuf::from(user_profile).join("Pictures").join("Snapdock");
    if !path.exists() {
        fs::create_dir_all(&path)?;
    }
    Ok(path)
}

pub fn generate_file_path() -> io::Result<PathBuf> {
    let dir = get_save_dir()?;
    let st = unsafe { GetLocalTime() };

    let base_name = format!(
        "snapdock-{:04}{:02}{:02}-{:02}{:02}{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    );

    let mut candidate = dir.join(format!("{base_name}.png"));
    if !candidate.exists() {
        return Ok(candidate);
    }

    // If exists in the same second, append subsecond / counter
    for i in 1..1000 {
        candidate = dir.join(format!("{base_name}_{i:03}.png"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Ok(dir.join(format!("{base_name}_{}.png", st.wMilliseconds)))
}

pub fn encode_png_buffer(rgba: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::with_capacity((width * height) as usize);
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut buffer), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);

        let mut writer = encoder
            .write_header()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        writer
            .write_image_data(rgba)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    }
    Ok(buffer)
}

pub fn save_png(rgba: &[u8], width: u32, height: u32) -> io::Result<(PathBuf, Vec<u8>)> {
    let png_bytes = encode_png_buffer(rgba, width, height)?;
    let path = generate_file_path()?;
    fs::write(&path, &png_bytes)?;
    Ok((path, png_bytes))
}
