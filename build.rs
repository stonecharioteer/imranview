use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/branding/favicon.png");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    configure_optional_turbojpeg_linking();

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

fn configure_optional_turbojpeg_linking() {
    if env::var_os("CARGO_FEATURE_TURBOJPEG").is_none() {
        return;
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" && target_os != "macos" {
        return;
    }

    let output = Command::new("pkg-config")
        .args(["--libs", "libturbojpeg"])
        .output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!(
                "turbojpeg feature enabled but pkg-config failed for libturbojpeg: {}",
                stderr.trim()
            );
        }
        Err(err) => {
            panic!("turbojpeg feature enabled but pkg-config is unavailable: {err}");
        }
    };

    let libs = String::from_utf8_lossy(&output.stdout);
    for token in libs.split_whitespace() {
        if let Some(path) = token.strip_prefix("-L") {
            if !path.is_empty() {
                println!("cargo:rustc-link-search=native={path}");
            }
            continue;
        }
        if let Some(lib) = token.strip_prefix("-l") {
            if !lib.is_empty() {
                println!("cargo:rustc-link-lib={lib}");
            }
            continue;
        }
    }
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
