use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/branding/favicon.png");

    if let Err(err) = prepare_executable_icon() {
        println!("cargo:warning=failed to prepare executable icon: {err}");
        #[cfg(target_os = "windows")]
        panic!("failed to prepare executable icon: {err}");
    }
}

fn prepare_executable_icon() -> Result<(), Box<dyn Error>> {
    let source_png = PathBuf::from("assets/branding/favicon.png");
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let icon_path = out_dir.join("imranview.ico");
    write_windows_icon(&source_png, &icon_path)?;

    #[cfg(target_os = "windows")]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(path_to_str(&icon_path)?);
        resource.compile()?;
    }

    Ok(())
}

fn write_windows_icon(source_png: &Path, icon_path: &Path) -> Result<(), Box<dyn Error>> {
    let source = image::open(source_png)?;
    let rgba = source.to_rgba8();
    let resized = image::imageops::resize(&rgba, 256, 256, image::imageops::FilterType::Lanczos3);
    image::DynamicImage::ImageRgba8(resized)
        .save_with_format(icon_path, image::ImageFormat::Ico)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn path_to_str(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str()
        .ok_or_else(|| format!("non-utf8 path: {}", path.display()).into())
}
