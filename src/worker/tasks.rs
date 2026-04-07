use super::*;

pub(super) fn run_capture_screenshot(
    request_id: u64,
    delay_ms: u64,
    region: Option<(u32, u32, u32, u32)>,
    output_path: Option<PathBuf>,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let path = output_path.unwrap_or_else(default_screenshot_path);
        capture_screenshot_to_path(&path, delay_ms, region)?;
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Captured screenshot {}", path.display()),
            open_path: Some(path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn run_open_tiff_page(request_id: u64, path: PathBuf, page_index: u32) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let (actual_page, page_count, image) = decode_tiff_page(&path, page_index)?;
        let loaded = payload_from_working_image(Arc::new(image));
        Ok(WorkerResult::TiffPageLoaded {
            request_id,
            page_index: actual_page,
            page_count,
            loaded,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn run_lossless_jpeg(
    request_id: u64,
    path: PathBuf,
    op: LosslessJpegOp,
    output_path: Option<PathBuf>,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "jpg" && ext != "jpeg" {
            return Err(anyhow!("lossless JPEG operations require .jpg/.jpeg input"));
        }
        if !path.is_file() {
            return Err(anyhow!("missing input file {}", path.display()));
        }

        let final_output = output_path.unwrap_or_else(|| path.clone());
        if let Some(parent) = final_output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let temp_output = if final_output == path {
            final_output.with_extension("jpg.tmp")
        } else {
            final_output.clone()
        };

        let mut command = std::process::Command::new("jpegtran");
        command.arg("-copy").arg("all");
        match op {
            LosslessJpegOp::Rotate90 => {
                command.arg("-rotate").arg("90");
            }
            LosslessJpegOp::Rotate180 => {
                command.arg("-rotate").arg("180");
            }
            LosslessJpegOp::Rotate270 => {
                command.arg("-rotate").arg("270");
            }
            LosslessJpegOp::FlipHorizontal => {
                command.arg("-flip").arg("horizontal");
            }
            LosslessJpegOp::FlipVertical => {
                command.arg("-flip").arg("vertical");
            }
        }
        command.arg("-outfile").arg(&temp_output).arg(&path);
        let status = command
            .status()
            .with_context(|| "failed to execute `jpegtran` (install libjpeg-turbo tools)")?;
        if !status.success() {
            return Err(anyhow!("jpegtran failed with status {status}"));
        }

        if final_output == path {
            fs::rename(&temp_output, &path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Lossless JPEG transform complete: {}",
                final_output.display()
            ),
            open_path: Some(final_output),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn run_update_exif_date(request_id: u64, path: PathBuf, datetime: String) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if !path.is_file() {
            return Err(anyhow!("missing input file {}", path.display()));
        }
        let datetime = datetime.trim();
        if datetime.is_empty() {
            return Err(anyhow!("datetime is required"));
        }
        let normalized = datetime.replace('T', " ");

        let status = std::process::Command::new("exiftool")
            .arg("-overwrite_original")
            .arg(format!("-DateTimeOriginal={normalized}"))
            .arg(format!("-CreateDate={normalized}"))
            .arg(format!("-ModifyDate={normalized}"))
            .arg(&path)
            .status()
            .with_context(|| "failed to execute `exiftool` (install exiftool and ensure PATH)")?;
        if !status.success() {
            return Err(anyhow!("exiftool failed with status {status}"));
        }
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Updated EXIF date/time for {}", path.display()),
            open_path: Some(path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn run_convert_color_profile(
    request_id: u64,
    path: PathBuf,
    output_path: PathBuf,
    source_profile: Option<PathBuf>,
    target_profile: PathBuf,
    rendering_intent: String,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if !path.is_file() {
            return Err(anyhow!("missing input image {}", path.display()));
        }
        if !target_profile.is_file() {
            return Err(anyhow!(
                "missing target profile {}",
                target_profile.display()
            ));
        }
        if let Some(source_profile) = source_profile.as_ref() {
            if !source_profile.is_file() {
                return Err(anyhow!(
                    "missing source profile {}",
                    source_profile.display()
                ));
            }
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut image = load_working_image(&path)
            .map_err(|err| anyhow!("failed to decode input image {}: {err}", path.display()))?
            .to_rgba8();

        let source_profile = if let Some(profile_path) = source_profile.as_ref() {
            Profile::new_file(profile_path).with_context(|| {
                format!(
                    "failed to load source ICC profile {}",
                    profile_path.display()
                )
            })?
        } else if let Some(embedded) = extract_embedded_icc_profile(&path).with_context(|| {
            format!(
                "failed to inspect embedded ICC profile in {}",
                path.display()
            )
        })? {
            Profile::new_icc(&embedded).with_context(|| {
                format!(
                    "failed to parse embedded ICC profile from {}",
                    path.display()
                )
            })?
        } else {
            Profile::new_srgb()
        };
        let target_profile = Profile::new_file(&target_profile).with_context(|| {
            format!(
                "failed to load target ICC profile {}",
                target_profile.display()
            )
        })?;

        let intent = match rendering_intent.trim().to_ascii_lowercase().as_str() {
            "perceptual" => Intent::Perceptual,
            "saturation" => Intent::Saturation,
            "absolute" | "absolutecolorimetric" | "absolute-colorimetric" => {
                Intent::AbsoluteColorimetric
            }
            _ => Intent::RelativeColorimetric,
        };

        let transform = Transform::new(
            &source_profile,
            PixelFormat::RGBA_8,
            &target_profile,
            PixelFormat::RGBA_8,
            intent,
        )
        .context("failed to build ICC transform")?;
        transform.transform_in_place(image.as_mut());

        let final_output = output_path;
        let temp_output = if final_output == path {
            temporary_icc_output_path(&path)
        } else {
            final_output.clone()
        };
        let save_format = infer_save_format(&temp_output, 92).map_err(|err| {
            anyhow!(
                "unsupported output extension for color-profile conversion {}: {err}",
                temp_output.display()
            )
        })?;
        let output_image = DynamicImage::ImageRgba8(image);
        save_image_with_format(&temp_output, &output_image, save_format).map_err(|err| {
            anyhow!(
                "failed to save color-profile output {}: {err}",
                temp_output.display()
            )
        })?;
        if let Ok(target_icc) = target_profile.icc() {
            if let Err(err) = embed_icc_profile_best_effort(&temp_output, save_format, &target_icc)
            {
                log::warn!(
                    target: "imranview::worker",
                    "failed to embed target ICC profile into {}: {err:#}",
                    temp_output.display()
                );
            }
        }

        if final_output == path {
            fs::rename(&temp_output, &path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Color profile conversion complete: {}",
                final_output.display()
            ),
            open_path: Some(final_output),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn temporary_icc_output_path(path: &Path) -> PathBuf {
    let ext = path
        .extension()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "png".to_owned());
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_owned());
    path.with_file_name(format!("{stem}.imranview.icc.tmp.{ext}"))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_scan_to_directory(
    request_id: u64,
    output_dir: PathBuf,
    output_format: BatchOutputFormat,
    rename_prefix: String,
    start_index: u32,
    page_count: u32,
    jpeg_quality: u8,
    command_template: String,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let template = command_template.trim();
        if template.is_empty() {
            return Err(anyhow!(
                "scanner command template is required (use {{output}} placeholder)"
            ));
        }
        let page_count = page_count.clamp(1, 10_000);
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;
        let extension = output_format
            .to_save_format(jpeg_quality.clamp(1, 100))
            .extension();

        let mut first_path: Option<PathBuf> = None;
        for page_offset in 0..page_count {
            let index = start_index.saturating_add(page_offset);
            let name = format!("{rename_prefix}{index:04}.{extension}");
            let output_path = output_dir.join(name);
            let command = template
                .replace("{output}", &shell_escape_path(&output_path))
                .replace("{index}", &index.to_string());
            run_shell_capture_command(&command).with_context(|| {
                format!(
                    "scanner command failed for output {}",
                    output_path.display()
                )
            })?;
            if !output_path.is_file() {
                return Err(anyhow!(
                    "scanner command completed but did not create {}",
                    output_path.display()
                ));
            }
            if first_path.is_none() {
                first_path = Some(output_path);
            }
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Captured {} scan page(s) to {}",
                page_count,
                output_dir.display()
            ),
            open_path: first_path,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_scan_native(
    request_id: u64,
    output_dir: PathBuf,
    output_format: BatchOutputFormat,
    rename_prefix: String,
    start_index: u32,
    page_count: u32,
    jpeg_quality: u8,
    dpi: u32,
    grayscale: bool,
    device_name: Option<String>,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let page_count = page_count.clamp(1, 10_000);
        let dpi = dpi.clamp(75, 1200);
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;

        let save_format = output_format.to_save_format(jpeg_quality.clamp(1, 100));
        let extension = save_format.extension();
        let mut first_path: Option<PathBuf> = None;

        for page_offset in 0..page_count {
            let index = start_index.saturating_add(page_offset);
            let final_name = format!("{rename_prefix}{index:04}.{extension}");
            let final_path = output_dir.join(final_name);
            let temp_capture = output_dir.join(format!(".imranview-scan-{index:04}.png"));

            scan_native_capture_to_png(&temp_capture, dpi, grayscale, device_name.as_deref())
                .with_context(|| {
                    format!("native scan capture failed for page {}", page_offset + 1)
                })?;

            let image = load_working_image(&temp_capture).with_context(|| {
                format!("failed to decode scanned image {}", temp_capture.display())
            })?;
            save_image_with_format(&final_path, &image, save_format)
                .with_context(|| format!("failed to save {}", final_path.display()))?;
            let _ = fs::remove_file(&temp_capture);

            if first_path.is_none() {
                first_path = Some(final_path.clone());
            }
        }

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Captured {} page(s) via native scanner backend to {}",
                page_count,
                output_dir.display()
            ),
            open_path: first_path,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn scan_native_capture_to_png(
    output_path: &Path,
    dpi: u32,
    grayscale: bool,
    device_name: Option<&str>,
) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mode = if grayscale { "Gray" } else { "Color" };
        let mut command = std::process::Command::new("scanimage");
        command
            .arg("--format=png")
            .arg("--mode")
            .arg(mode)
            .arg("--resolution")
            .arg(dpi.to_string())
            .arg("--output-file")
            .arg(output_path);
        if let Some(device_name) = device_name.filter(|value| !value.trim().is_empty()) {
            command.arg("--device-name").arg(device_name.trim());
        }
        let status = command
            .status()
            .with_context(|| "failed to execute `scanimage` (install SANE tools)")?;
        if !status.success() {
            let mut message = format!("scanimage failed with status {status}");
            if let Ok(devices) = scanimage_list_devices() {
                if !devices.trim().is_empty() {
                    message.push_str("\nDetected scanner devices:\n");
                    message.push_str(devices.trim());
                }
            }
            return Err(anyhow!(message));
        }
        if !output_path.is_file() {
            return Err(anyhow!(
                "scanimage completed but did not produce {}",
                output_path.display()
            ));
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let _ = device_name;
        let escaped = output_path.display().to_string().replace('\'', "''");
        let wia_format = if grayscale {
            "{B96B3CAA-0728-11D3-9D7B-0000F81EF32E}" // PNG
        } else {
            "{B96B3CAF-0728-11D3-9D7B-0000F81EF32E}" // JPEG
        };
        let script = format!(
            "$dialog=New-Object -ComObject WIA.CommonDialog; \
             $device=$dialog.ShowSelectDevice(1,$true,$false); \
             if($null -eq $device){{exit 2}}; \
             $item=$device.Items.Item(1); \
             $item.Properties.Item('6147').Value={dpi}; \
             $item.Properties.Item('6148').Value={dpi}; \
             $img=$dialog.ShowTransfer($item,'{wia_format}',$false); \
             if($null -eq $img){{exit 3}}; \
             $img.SaveFile('{escaped}');"
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status()
            .with_context(|| "failed to execute Windows WIA scanner command")?;
        if !status.success() {
            return Err(anyhow!(
                "Windows WIA scanner command failed with status {status}"
            ));
        }
        if !output_path.is_file() {
            return Err(anyhow!(
                "WIA command completed but did not produce {}",
                output_path.display()
            ));
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (output_path, dpi, grayscale, device_name);
        Err(anyhow!(
            "native scanner backend is not supported on this platform"
        ))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn scanimage_list_devices() -> Result<String> {
    let output = std::process::Command::new("scanimage")
        .arg("-L")
        .output()
        .with_context(|| "failed to execute `scanimage -L` for device listing")?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stderr).into_owned())
}

pub(super) fn shell_escape_path(path: &Path) -> String {
    #[cfg(target_os = "windows")]
    {
        return format!("\"{}\"", path.display().to_string().replace('"', "\"\""));
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("'{}'", path.display().to_string().replace('\'', "'\"'\"'"))
    }
}

pub(super) fn run_shell_capture_command(command: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", command])
        .status()
        .with_context(|| format!("failed to launch command: {command}"))?;

    #[cfg(not(target_os = "windows"))]
    let status = std::process::Command::new("sh")
        .args(["-c", command])
        .status()
        .with_context(|| format!("failed to launch command: {command}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("command exited with status {status}: {command}"))
    }
}

pub(super) fn run_extract_tiff_pages(
    request_id: u64,
    path: PathBuf,
    output_dir: PathBuf,
    output_format: BatchOutputFormat,
    jpeg_quality: u8,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        let pages = decode_tiff_pages(&path)?;
        if pages.is_empty() {
            return Err(anyhow!("no pages found in {}", path.display()));
        }
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;

        let save_format = output_format.to_save_format(jpeg_quality.clamp(1, 100));
        let extension = save_format.extension();
        for (index, image) in pages.iter().enumerate() {
            let file_name = format!("page-{:04}.{extension}", index + 1);
            let output_path = output_dir.join(file_name);
            save_image_with_format(&output_path, image, save_format)
                .with_context(|| format!("failed to save {}", output_path.display()))?;
        }
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Extracted {} TIFF page(s) to {}",
                pages.len(),
                output_dir.display()
            ),
            open_path: None,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn run_create_multipage_pdf(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_path: PathBuf,
    jpeg_quality: u8,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.is_empty() {
            return Err(anyhow!("select at least one image for PDF creation"));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut pages = Vec::with_capacity(input_paths.len());
        for path in &input_paths {
            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            pages.push(pdf_page_from_image(&image, jpeg_quality.clamp(1, 100))?);
        }
        let pdf = build_simple_pdf(&pages)?;
        fs::write(&output_path, pdf)
            .with_context(|| format!("failed to write {}", output_path.display()))?;

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!(
                "Created PDF with {} page(s): {}",
                input_paths.len(),
                output_path.display()
            ),
            open_path: None,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn run_ocr(
    request_id: u64,
    path: PathBuf,
    language: String,
    output_path: Option<PathBuf>,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if !path.exists() {
            return Err(anyhow!("cannot OCR missing file {}", path.display()));
        }
        let language = if language.trim().is_empty() {
            "eng".to_owned()
        } else {
            language.trim().to_owned()
        };
        let command = std::process::Command::new("tesseract")
            .arg(&path)
            .arg("stdout")
            .arg("-l")
            .arg(&language)
            .output();
        let command = match command {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(anyhow!(
                    "`tesseract` is not installed or not on PATH. Install it, then retry OCR."
                ));
            }
            Err(error) => {
                return Err(error).with_context(
                    || "failed to execute `tesseract` (install it and ensure it is on PATH)",
                );
            }
        };
        if !command.status.success() {
            let stderr = String::from_utf8_lossy(&command.stderr);
            let mut message = format!("tesseract failed: {}", stderr.trim());
            if let Ok(languages) = tesseract_list_languages() {
                if !languages.is_empty() {
                    message.push_str("\nDetected tesseract languages: ");
                    message.push_str(&languages.join(", "));
                }
            }
            return Err(anyhow!(message));
        }
        let text = String::from_utf8(command.stdout).context("OCR output is not valid UTF-8")?;
        if let Some(path) = output_path.as_ref() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(path, &text)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        Ok(WorkerResult::OcrCompleted {
            request_id,
            output_path,
            text,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Ocr,
        error: err.to_string(),
    })
}

pub(super) fn tesseract_list_languages() -> Result<Vec<String>> {
    let output = std::process::Command::new("tesseract")
        .arg("--list-langs")
        .output()
        .with_context(|| "failed to execute `tesseract --list-langs`")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut languages = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.ends_with(':') {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("list of available languages") {
            continue;
        }
        languages.push(trimmed.to_owned());
    }
    languages.sort();
    languages.dedup();
    Ok(languages)
}

pub(super) fn run_stitch_panorama(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_path: PathBuf,
    direction: PanoramaDirection,
    overlap_percent: f32,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.len() < 2 {
            return Err(anyhow!("select at least two images for panorama"));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut images = Vec::with_capacity(input_paths.len());
        for path in &input_paths {
            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            images.push(image.to_rgba8());
        }
        let stitched = stitch_images(&images, direction, overlap_percent.clamp(0.0, 0.9));
        let stitched = DynamicImage::ImageRgba8(stitched);
        let save_format = infer_save_format(&output_path, 90).unwrap_or(SaveFormat::Png);
        save_image_with_format(&output_path, &stitched, save_format)
            .with_context(|| format!("failed to save {}", output_path.display()))?;
        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Panorama written to {}", output_path.display()),
            open_path: Some(output_path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_export_contact_sheet(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_path: PathBuf,
    columns: u32,
    thumb_size: u32,
    include_labels: bool,
    background: [u8; 4],
    label_color: [u8; 4],
    jpeg_quality: u8,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.is_empty() {
            return Err(anyhow!("no input images selected for contact sheet"));
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let columns = columns.max(1);
        let thumb_size = thumb_size.clamp(32, 1024);
        let rows = ((input_paths.len() as f32) / columns as f32).ceil() as u32;
        let label_height = if include_labels { 18 } else { 0 };
        let card_w = thumb_size + 16;
        let card_h = thumb_size + 16 + label_height;
        let sheet_w = (columns * card_w).max(1);
        let sheet_h = (rows * card_h).max(1);
        let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba(background));

        for (index, path) in input_paths.iter().enumerate() {
            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            let thumb = image.thumbnail(thumb_size, thumb_size).to_rgba8();
            let col = index as u32 % columns;
            let row = index as u32 / columns;
            let base_x = col * card_w;
            let base_y = row * card_h;
            let x = base_x + (card_w.saturating_sub(thumb.width())) / 2;
            let y = base_y + 8 + (thumb_size.saturating_sub(thumb.height())) / 2;
            blit_rgba(&thumb, &mut sheet, x, y);

            if include_labels {
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                draw_bitmap_text(
                    &mut sheet,
                    &label,
                    (base_x + 6) as i32,
                    (base_y + thumb_size + 10) as i32,
                    1,
                    Rgba(label_color),
                );
            }
        }

        let save_format =
            infer_save_format(&output_path, jpeg_quality.clamp(1, 100)).unwrap_or(SaveFormat::Png);
        let output = DynamicImage::ImageRgba8(sheet);
        save_image_with_format(&output_path, &output, save_format)
            .with_context(|| format!("failed to save {}", output_path.display()))?;

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("Contact sheet exported to {}", output_path.display()),
            open_path: Some(output_path),
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn run_export_html_gallery(
    request_id: u64,
    input_paths: Vec<PathBuf>,
    output_dir: PathBuf,
    title: String,
    thumb_width: u32,
) -> WorkerResult {
    let output = (|| -> Result<WorkerResult> {
        if input_paths.is_empty() {
            return Err(anyhow!("no input images selected for HTML export"));
        }
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;
        let thumbs_dir = output_dir.join("thumbs");
        let images_dir = output_dir.join("images");
        fs::create_dir_all(&thumbs_dir)
            .with_context(|| format!("failed to create {}", thumbs_dir.display()))?;
        fs::create_dir_all(&images_dir)
            .with_context(|| format!("failed to create {}", images_dir.display()))?;

        let mut items = Vec::with_capacity(input_paths.len());
        for (index, path) in input_paths.iter().enumerate() {
            let source_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("image-{index:04}.png"));
            let image_target = images_dir.join(format!("{index:04}-{source_name}"));
            fs::copy(path, &image_target).with_context(|| {
                format!(
                    "failed to copy source image {} to {}",
                    path.display(),
                    image_target.display()
                )
            })?;

            let image = load_working_image(path)
                .map_err(|err| anyhow!("failed to load {}: {err}", path.display()))?;
            let thumb = image.thumbnail(thumb_width.max(32), thumb_width.max(32));
            let thumb_name = format!("{index:04}.jpg");
            let thumb_target = thumbs_dir.join(&thumb_name);
            save_image_with_format(&thumb_target, &thumb, SaveFormat::Jpeg { quality: 90 })
                .with_context(|| format!("failed to save {}", thumb_target.display()))?;

            items.push((
                source_name,
                format!(
                    "images/{}",
                    image_target.file_name().unwrap().to_string_lossy()
                ),
                format!("thumbs/{thumb_name}"),
            ));
        }

        let safe_title = if title.trim().is_empty() {
            "ImranView Gallery".to_owned()
        } else {
            title.trim().to_owned()
        };
        let mut html = String::new();
        html.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
        html.push_str(&format!("<title>{}</title>", html_escape(&safe_title)));
        html.push_str(
            "<style>body{font-family:system-ui,sans-serif;margin:24px;background:#111;color:#f3f3f3}h1{font-size:20px;margin:0 0 16px}\
            .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:14px}\
            .card{background:#1d1d1d;border:1px solid #303030;border-radius:8px;padding:8px}\
            .card img{width:100%;height:auto;display:block;border-radius:4px}\
            .name{font-size:12px;overflow-wrap:anywhere;margin-top:8px;color:#ddd}</style></head><body>",
        );
        html.push_str(&format!(
            "<h1>{}</h1><div class=\"grid\">",
            html_escape(&safe_title)
        ));
        for (name, image_href, thumb_href) in items {
            html.push_str("<a class=\"card\" href=\"");
            html.push_str(&html_escape(&image_href));
            html.push_str("\"><img src=\"");
            html.push_str(&html_escape(&thumb_href));
            html.push_str("\" alt=\"");
            html.push_str(&html_escape(&name));
            html.push_str("\"><div class=\"name\">");
            html.push_str(&html_escape(&name));
            html.push_str("</div></a>");
        }
        html.push_str("</div></body></html>");

        let index_path = output_dir.join("index.html");
        fs::write(&index_path, html)
            .with_context(|| format!("failed to write {}", index_path.display()))?;

        Ok(WorkerResult::UtilityCompleted {
            request_id,
            message: format!("HTML gallery exported to {}", index_path.display()),
            open_path: None,
        })
    })();

    output.unwrap_or_else(|err| WorkerResult::Failed {
        request_id: Some(request_id),
        kind: WorkerRequestKind::Utility,
        error: err.to_string(),
    })
}

pub(super) fn default_screenshot_path() -> PathBuf {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("imranview-screenshot-{timestamp_ms}.png"))
}

pub(super) fn capture_screenshot_to_path(
    path: &Path,
    delay_ms: u64,
    region: Option<(u32, u32, u32, u32)>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if delay_ms > 0 {
        thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("screencapture");
        command.arg("-x");
        if let Some((x, y, width, height)) = region {
            command.arg(format!("-R{x},{y},{width},{height}"));
        }
        command.arg(path);
        let status = command
            .status()
            .context("failed to execute macOS screencapture command")?;
        if !status.success() {
            return Err(anyhow!("screencapture failed with status {}", status));
        }
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let run_and_check = |mut command: std::process::Command,
                             label: &str|
         -> Result<Option<std::process::ExitStatus>> {
            match command.status() {
                Ok(status) => Ok(Some(status)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(anyhow!("failed to execute {label}: {err}")),
            }
        };

        if let Some((x, y, width, height)) = region {
            let mut grim = std::process::Command::new("grim");
            grim.arg("-g")
                .arg(format!("{x},{y} {width}x{height}"))
                .arg(path);
            if let Some(status) = run_and_check(grim, "grim")? {
                if status.success() {
                    return Ok(());
                }
                return Err(anyhow!("grim failed with status {}", status));
            }
        } else {
            let mut grim = std::process::Command::new("grim");
            grim.arg(path);
            if let Some(status) = run_and_check(grim, "grim")? {
                if status.success() {
                    return Ok(());
                }
                return Err(anyhow!("grim failed with status {}", status));
            }
        }

        let mut gnome = std::process::Command::new("gnome-screenshot");
        gnome.arg("-f").arg(path);
        if delay_ms > 0 {
            gnome
                .arg("-d")
                .arg((delay_ms as f32 / 1000.0).ceil().to_string());
        }
        if let Some(status) = run_and_check(gnome, "gnome-screenshot")? {
            if status.success() {
                if let Some(region) = region {
                    crop_image_file_in_place(path, region)?;
                }
                return Ok(());
            }
            return Err(anyhow!("gnome-screenshot failed with status {}", status));
        }

        let mut import = std::process::Command::new("import");
        import.arg("-window").arg("root").arg(path);
        if let Some(status) = run_and_check(import, "import")? {
            if status.success() {
                if let Some(region) = region {
                    crop_image_file_in_place(path, region)?;
                }
                return Ok(());
            }
            return Err(anyhow!("import failed with status {}", status));
        }

        let mut scrot = std::process::Command::new("scrot");
        scrot.arg(path);
        if delay_ms > 0 {
            scrot
                .arg("-d")
                .arg((delay_ms as f32 / 1000.0).ceil().to_string());
        }
        if let Some(status) = run_and_check(scrot, "scrot")? {
            if status.success() {
                if let Some(region) = region {
                    crop_image_file_in_place(path, region)?;
                }
                return Ok(());
            }
            return Err(anyhow!("scrot failed with status {}", status));
        }

        return Err(anyhow!(
            "no screenshot backend found (install grim, gnome-screenshot, import, or scrot)"
        ));
    }

    #[cfg(target_os = "windows")]
    {
        let escaped_path = path.display().to_string().replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             Add-Type -AssemblyName System.Drawing; \
             $bounds=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
             $bitmap=New-Object System.Drawing.Bitmap($bounds.Width,$bounds.Height); \
             $graphics=[System.Drawing.Graphics]::FromImage($bitmap); \
             $graphics.CopyFromScreen($bounds.Location,[System.Drawing.Point]::Empty,$bounds.Size); \
             $bitmap.Save('{escaped_path}', [System.Drawing.Imaging.ImageFormat]::Png); \
             $graphics.Dispose(); \
             $bitmap.Dispose();"
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status()
            .context("failed to execute PowerShell screenshot command")?;
        if !status.success() {
            return Err(anyhow!(
                "PowerShell screenshot failed with status {}",
                status
            ));
        }
        if let Some(region) = region {
            crop_image_file_in_place(path, region)?;
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (path, region);
        return Err(anyhow!(
            "screenshot capture is not supported on this platform"
        ));
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) fn crop_image_file_in_place(path: &Path, region: (u32, u32, u32, u32)) -> Result<()> {
    let (x, y, width, height) = region;
    if width == 0 || height == 0 {
        return Ok(());
    }
    let image = image::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let (source_w, source_h) = image.dimensions();
    if x >= source_w || y >= source_h {
        return Ok(());
    }
    let crop_w = width.min(source_w - x);
    let crop_h = height.min(source_h - y);
    let cropped = image.crop_imm(x, y, crop_w, crop_h);
    cropped
        .save(path)
        .with_context(|| format!("failed to save cropped screenshot {}", path.display()))
}

pub(super) fn decode_tiff_page(path: &Path, requested_page: u32) -> Result<(u32, u32, DynamicImage)> {
    let pages = decode_tiff_pages(path)?;
    if pages.is_empty() {
        return Err(anyhow!("TIFF has no decodable pages"));
    }
    let page_count = pages.len() as u32;
    let actual_page = requested_page.min(page_count.saturating_sub(1));
    let image = pages
        .into_iter()
        .nth(actual_page as usize)
        .context("failed to fetch requested TIFF page")?;
    Ok((actual_page, page_count, image))
}

pub(super) fn decode_tiff_pages(path: &Path) -> Result<Vec<DynamicImage>> {
    let file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut decoder = tiff::decoder::Decoder::new(std::io::BufReader::new(file))
        .with_context(|| format!("failed to decode TIFF headers {}", path.display()))?;
    let mut pages = Vec::new();

    loop {
        let (width, height) = decoder
            .dimensions()
            .with_context(|| format!("failed to read dimensions for {}", path.display()))?;
        let color_type = decoder
            .colortype()
            .with_context(|| format!("failed to read color type for {}", path.display()))?;
        let decoded = decoder
            .read_image()
            .with_context(|| format!("failed to decode TIFF page for {}", path.display()))?;
        let rgba = tiff_decoding_to_rgba(decoded, color_type, width, height)?;
        pages.push(DynamicImage::ImageRgba8(rgba));

        if !decoder.more_images() {
            break;
        }
        decoder
            .next_image()
            .with_context(|| format!("failed to read next TIFF page in {}", path.display()))?;
    }

    Ok(pages)
}

pub(super) fn tiff_decoding_to_rgba(
    decoded: tiff::decoder::DecodingResult,
    color_type: tiff::ColorType,
    width: u32,
    height: u32,
) -> Result<RgbaImage> {
    let pixel_count = width as usize * height as usize;
    let mut rgba = vec![0u8; pixel_count * 4];
    match decoded {
        tiff::decoder::DecodingResult::U8(buffer) => match color_type {
            tiff::ColorType::Gray(_) => {
                if buffer.len() < pixel_count {
                    return Err(anyhow!("invalid TIFF grayscale buffer length"));
                }
                for index in 0..pixel_count {
                    let g = buffer[index];
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::GrayA(_) => {
                if buffer.len() < pixel_count * 2 {
                    return Err(anyhow!("invalid TIFF gray+alpha buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 2;
                    let g = buffer[base];
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = buffer[base + 1];
                }
            }
            tiff::ColorType::RGB(_) => {
                if buffer.len() < pixel_count * 3 {
                    return Err(anyhow!("invalid TIFF RGB buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 3;
                    rgba[index * 4] = buffer[base];
                    rgba[index * 4 + 1] = buffer[base + 1];
                    rgba[index * 4 + 2] = buffer[base + 2];
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::RGBA(_) => {
                if buffer.len() < pixel_count * 4 {
                    return Err(anyhow!("invalid TIFF RGBA buffer length"));
                }
                rgba.copy_from_slice(&buffer[..pixel_count * 4]);
            }
            other => {
                return Err(anyhow!("unsupported TIFF color type: {:?}", other));
            }
        },
        tiff::decoder::DecodingResult::U16(buffer) => match color_type {
            tiff::ColorType::Gray(_) => {
                if buffer.len() < pixel_count {
                    return Err(anyhow!("invalid TIFF grayscale16 buffer length"));
                }
                for index in 0..pixel_count {
                    let g = (buffer[index] >> 8) as u8;
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::GrayA(_) => {
                if buffer.len() < pixel_count * 2 {
                    return Err(anyhow!("invalid TIFF gray16+alpha16 buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 2;
                    let g = (buffer[base] >> 8) as u8;
                    rgba[index * 4] = g;
                    rgba[index * 4 + 1] = g;
                    rgba[index * 4 + 2] = g;
                    rgba[index * 4 + 3] = (buffer[base + 1] >> 8) as u8;
                }
            }
            tiff::ColorType::RGB(_) => {
                if buffer.len() < pixel_count * 3 {
                    return Err(anyhow!("invalid TIFF RGB16 buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 3;
                    rgba[index * 4] = (buffer[base] >> 8) as u8;
                    rgba[index * 4 + 1] = (buffer[base + 1] >> 8) as u8;
                    rgba[index * 4 + 2] = (buffer[base + 2] >> 8) as u8;
                    rgba[index * 4 + 3] = 255;
                }
            }
            tiff::ColorType::RGBA(_) => {
                if buffer.len() < pixel_count * 4 {
                    return Err(anyhow!("invalid TIFF RGBA16 buffer length"));
                }
                for index in 0..pixel_count {
                    let base = index * 4;
                    rgba[index * 4] = (buffer[base] >> 8) as u8;
                    rgba[index * 4 + 1] = (buffer[base + 1] >> 8) as u8;
                    rgba[index * 4 + 2] = (buffer[base + 2] >> 8) as u8;
                    rgba[index * 4 + 3] = (buffer[base + 3] >> 8) as u8;
                }
            }
            other => {
                return Err(anyhow!("unsupported TIFF color type: {:?}", other));
            }
        },
        other => {
            return Err(anyhow!(
                "unsupported TIFF sample type for decode: {:?}",
                other
            ));
        }
    }

    RgbaImage::from_raw(width, height, rgba).context("failed to construct RGBA TIFF page")
}

pub(super) struct PdfPageData {
    width: u32,
    height: u32,
    jpeg_bytes: Vec<u8>,
}

pub(super) fn pdf_page_from_image(image: &DynamicImage, jpeg_quality: u8) -> Result<PdfPageData> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut jpeg = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, jpeg_quality.clamp(1, 100));
    encoder
        .encode_image(&DynamicImage::ImageRgb8(rgb))
        .context("failed to encode JPEG page for PDF")?;
    Ok(PdfPageData {
        width,
        height,
        jpeg_bytes: jpeg,
    })
}

pub(super) fn build_simple_pdf(pages: &[PdfPageData]) -> Result<Vec<u8>> {
    if pages.is_empty() {
        return Err(anyhow!("cannot build PDF without pages"));
    }

    let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut kids = String::new();
    let mut next_id = 3u32;

    for (index, page) in pages.iter().enumerate() {
        let page_id = next_id;
        let content_id = next_id + 1;
        let image_id = next_id + 2;
        next_id += 3;

        if !kids.is_empty() {
            kids.push(' ');
        }
        kids.push_str(&format!("{page_id} 0 R"));

        let width_pt = ((page.width as f32) * 72.0 / 96.0).max(1.0);
        let height_pt = ((page.height as f32) * 72.0 / 96.0).max(1.0);
        let image_name = format!("Im{}", index + 1);
        let content_stream = format!(
            "q\n{} 0 0 {} 0 0 cm\n/{} Do\nQ\n",
            format_pdf_num(width_pt),
            format_pdf_num(height_pt),
            image_name
        );
        let mut content_obj =
            format!("<< /Length {} >>\nstream\n", content_stream.len()).into_bytes();
        content_obj.extend_from_slice(content_stream.as_bytes());
        content_obj.extend_from_slice(b"endstream\n");
        objects.push((content_id, content_obj));

        let mut image_obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.jpeg_bytes.len()
        )
        .into_bytes();
        image_obj.extend_from_slice(&page.jpeg_bytes);
        image_obj.extend_from_slice(b"\nendstream\n");
        objects.push((image_id, image_obj));

        let page_obj = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /XObject << /{} {} 0 R >> >> /Contents {} 0 R >>\n",
            format_pdf_num(width_pt),
            format_pdf_num(height_pt),
            image_name,
            image_id,
            content_id
        )
        .into_bytes();
        objects.push((page_id, page_obj));
    }

    objects.push((
        2,
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>\n",
            pages.len(),
            kids
        )
        .into_bytes(),
    ));
    objects.push((1, b"<< /Type /Catalog /Pages 2 0 R >>\n".to_vec()));

    objects.sort_by_key(|(id, _)| *id);
    let max_id = objects
        .iter()
        .map(|(id, _)| *id)
        .max()
        .context("missing PDF objects")?;

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = vec![0usize; max_id as usize + 1];
    for (id, object) in objects {
        offsets[id as usize] = pdf.len();
        pdf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        pdf.extend_from_slice(&object);
        pdf.extend_from_slice(b"endobj\n");
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for object_id in 1..=max_id {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", offsets[object_id as usize]).as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            max_id + 1,
            xref_offset
        )
        .as_bytes(),
    );
    Ok(pdf)
}

pub(super) fn format_pdf_num(value: f32) -> String {
    format!("{:.3}", value)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
