use std::ffi::CStr;
use std::fs;
use std::os::raw::{c_char, c_int, c_uchar, c_ulong, c_void};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, RgbaImage};

#[cfg(all(feature = "turbojpeg", any(target_os = "linux", target_os = "macos")))]
mod ffi {
    use super::{c_char, c_int, c_uchar, c_ulong, c_void};

    pub type TjHandle = *mut c_void;

    pub const TJPF_RGBA: c_int = 7;
    pub const TJFLAG_FASTDCT: c_int = 2048;
    pub const TJFLAG_FASTUPSAMPLE: c_int = 256;

    unsafe extern "C" {
        pub fn tjInitDecompress() -> TjHandle;
        pub fn tjDestroy(handle: TjHandle) -> c_int;
        pub fn tjDecompressHeader3(
            handle: TjHandle,
            jpeg_buf: *const c_uchar,
            jpeg_size: c_ulong,
            width: *mut c_int,
            height: *mut c_int,
            jpeg_subsamp: *mut c_int,
            jpeg_colorspace: *mut c_int,
        ) -> c_int;
        pub fn tjDecompress2(
            handle: TjHandle,
            jpeg_buf: *const c_uchar,
            jpeg_size: c_ulong,
            dst_buf: *mut c_uchar,
            width: c_int,
            pitch: c_int,
            height: c_int,
            pixel_format: c_int,
            flags: c_int,
        ) -> c_int;
        pub fn tjGetErrorStr2(handle: TjHandle) -> *mut c_char;
    }
}

#[cfg(all(feature = "turbojpeg", any(target_os = "linux", target_os = "macos")))]
struct TurboJpegDecompressor(ffi::TjHandle);

#[cfg(all(feature = "turbojpeg", any(target_os = "linux", target_os = "macos")))]
impl Drop for TurboJpegDecompressor {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: handle is allocated by tjInitDecompress and owned by this wrapper.
        unsafe {
            let _ = ffi::tjDestroy(self.0);
        }
    }
}

#[cfg(all(feature = "turbojpeg", any(target_os = "linux", target_os = "macos")))]
fn turbojpeg_error(handle: ffi::TjHandle, context: &str) -> anyhow::Error {
    // SAFETY: tjGetErrorStr2 returns a valid null-terminated static/error buffer.
    let detail = unsafe {
        let ptr = ffi::tjGetErrorStr2(handle);
        if ptr.is_null() {
            None
        } else {
            Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
        }
    }
    .unwrap_or_else(|| "unknown turbojpeg error".to_owned());
    anyhow!("{context}: {detail}")
}

#[cfg(all(feature = "turbojpeg", any(target_os = "linux", target_os = "macos")))]
pub fn decode_jpeg_with_turbojpeg(path: &Path) -> Result<DynamicImage> {
    let jpeg = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;

    // SAFETY: calling C API with validated pointers and lengths.
    unsafe {
        let handle = ffi::tjInitDecompress();
        if handle.is_null() {
            return Err(anyhow!(
                "tjInitDecompress returned null (libturbojpeg unavailable?)"
            ));
        }
        let handle = TurboJpegDecompressor(handle);

        let mut width = 0;
        let mut height = 0;
        let mut subsamp = 0;
        let mut colorspace = 0;
        if ffi::tjDecompressHeader3(
            handle.0,
            jpeg.as_ptr(),
            jpeg.len() as c_ulong,
            &mut width,
            &mut height,
            &mut subsamp,
            &mut colorspace,
        ) != 0
        {
            return Err(turbojpeg_error(handle.0, "tjDecompressHeader3 failed"));
        }
        if width <= 0 || height <= 0 {
            return Err(anyhow!(
                "invalid jpeg dimensions width={width} height={height}"
            ));
        }

        let pixel_count = (width as usize)
            .checked_mul(height as usize)
            .context("pixel count overflow while decoding jpeg")?;
        let buffer_len = pixel_count
            .checked_mul(4)
            .context("RGBA buffer overflow while decoding jpeg")?;
        let mut rgba = vec![0u8; buffer_len];

        let flags = ffi::TJFLAG_FASTUPSAMPLE | ffi::TJFLAG_FASTDCT;
        if ffi::tjDecompress2(
            handle.0,
            jpeg.as_ptr(),
            jpeg.len() as c_ulong,
            rgba.as_mut_ptr(),
            width,
            0,
            height,
            ffi::TJPF_RGBA,
            flags,
        ) != 0
        {
            return Err(turbojpeg_error(handle.0, "tjDecompress2 failed"));
        }

        let image = RgbaImage::from_raw(width as u32, height as u32, rgba)
            .context("failed to construct image from turbojpeg output")?;
        Ok(DynamicImage::ImageRgba8(image))
    }
}

#[cfg(not(all(feature = "turbojpeg", any(target_os = "linux", target_os = "macos"))))]
pub fn decode_jpeg_with_turbojpeg(_path: &Path) -> Result<DynamicImage> {
    Err(anyhow!(
        "turbojpeg backend unavailable on this build (requires feature=turbojpeg on macOS/Linux)"
    ))
}
