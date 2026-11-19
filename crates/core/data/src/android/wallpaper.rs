//! [`WallpaperAdapter`] on Android: the lock screen's size and cropping the daily image
//! (WALL-06, SEC-02, ADR 0028). Setting the wallpaper arrives with #29. Until then, the
//! lock-screen image reports itself unavailable, so nothing changes on the device.
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here.
//!
//! `crop_image` blocks for as long as decoding takes, so it must run on a JVM thread other than
//! the main one, as `NetworkAdapter::get` does. The bridge refuses to crop on the main thread.

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use super::not_yet;
use crate::adapters::WallpaperAdapter;
use crate::model::{DataError, ImageCrop, Size};

pub struct AndroidWallpaper {
    vm: Vm,
    bridge: GlobalRef,
    /// `int[] lockScreenSize()`: the width and height, or `null`.
    lock_screen_size: MethodId,
    /// `byte[] cropImage(byte[] jpeg, int width, int height, int left, int top, int right,
    /// int bottom, int sample, int outWidth, int outHeight)`: the cropped image as a JPEG, or
    /// `null` if it couldn't be made.
    crop_image: MethodId,
}

impl AndroidWallpaper {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            lock_screen_size: env.method_id(bridge, c"lockScreenSize", c"()[I")?,
            crop_image: env.method_id(bridge, c"cropImage", c"([BIIIIIIIII)[B")?,
            bridge: env.global_ref(bridge)?,
        })
    }
}

impl WallpaperAdapter for AndroidWallpaper {
    /// Setting the lock-screen wallpaper arrives with #29.
    fn lock_screen_support(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    /// The default display's size in its natural orientation, from its mode, so turning the
    /// device doesn't change it. Empty if the bridge can't answer.
    fn lock_screen_size(&self) -> Size {
        let answer = self
            .vm
            .env()
            .and_then(|env| env.call_ints(&self.bridge, self.lock_screen_size, &[]));
        let pixels = |value: i32| u32::try_from(value).unwrap_or(0);
        match answer.ok().flatten().as_deref() {
            Some(&[width, height]) => Size {
                width: pixels(width),
                height: pixels(height),
            },
            _ => Size::default(),
        }
    }

    /// `BitmapRegionDecoder` decodes the region at 1/`sample` of its size, the result is scaled
    /// to `crop.size`, and `Bitmap.compress` writes it as a JPEG, so no metadata is kept.
    fn crop_image(&self, jpeg: &[u8], crop: &ImageCrop) -> Result<Vec<u8>, DataError> {
        let int = |value: u32| {
            i32::try_from(value)
                .map(JValue::int)
                .map_err(|_| unreadable())
        };
        let env = self.vm.env()?;
        let image = env.new_bytes(jpeg)?;
        let args = [
            JValue::object(&image),
            int(crop.image.width)?,
            int(crop.image.height)?,
            int(crop.left)?,
            int(crop.top)?,
            int(crop.left.saturating_add(crop.width))?,
            int(crop.top.saturating_add(crop.height))?,
            int(crop.sample)?,
            int(crop.size.width)?,
            int(crop.size.height)?,
        ];
        env.call_bytes(&self.bridge, self.crop_image, &args)?
            .ok_or_else(unreadable)
    }

    fn set_lock_screen(&self, _image: &[u8]) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn reset_lock_screen(&self) -> Result<(), DataError> {
        Err(not_yet())
    }

    fn open_wallpaper_settings(&self) -> Result<(), DataError> {
        Err(not_yet())
    }
}

/// The bridge couldn't decode the image, or not at the size its header gives.
fn unreadable() -> DataError {
    DataError::Invalid("The image couldn't be read".into())
}
