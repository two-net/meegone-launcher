//! [`WallpaperAdapter`] on Android: the device's screen, cropping the daily image, and the
//! lock-screen wallpaper itself: whether it can be set, setting it, which one the lock screen
//! has, and resetting it (WALL-01, WALL-06, WALL-12 to WALL-14, SEC-02, ADR 0028, ADR 0030).
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here. `WallpaperManager` sets only the lock screen's wallpaper
//! (`FLAG_LOCK`); the home wallpaper and the system lock screen itself are never touched
//! (SEC-05).
//!
//! `crop_image` and `set_lock_screen` block for as long as decoding or writing takes, so they
//! must run on a JVM thread other than the main one, as `NetworkAdapter::get` does. The bridge
//! refuses both on the main thread.

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use crate::adapters::WallpaperAdapter;
use crate::model::{DataError, ImageCrop, Screen, Size, WallpaperMarker};

/// What `lockScreenWallpaperId` answers when the system can't say.
const UNKNOWN_ID: i32 = i32::MIN;

pub struct AndroidWallpaper {
    vm: Vm,
    bridge: GlobalRef,
    /// `String lockScreenUnsupported()`: `null`, or why the wallpaper can't be set.
    lock_screen_unsupported: MethodId,
    /// `int[] screen()`: the default display's width, height and density, or `null`.
    screen: MethodId,
    /// `boolean folds()`: whether the device has a hinge.
    folds: MethodId,
    /// `byte[] cropImage(byte[] jpeg, int width, int height, int left, int top, int right,
    /// int bottom, int sample, int outWidth, int outHeight)`: the cropped image as a JPEG, or
    /// `null` if it couldn't be made.
    crop_image: MethodId,
    /// `int setLockScreen(byte[] jpeg)`: the new wallpaper's id, or -1 if it couldn't be set.
    set_lock_screen: MethodId,
    /// `int lockScreenWallpaperId()`: the lock screen's wallpaper id, negative for none of its
    /// own, or `Integer.MIN_VALUE` if the system can't say.
    lock_screen_wallpaper_id: MethodId,
    /// `String resetLockScreen()`: `null`, or a short message when it couldn't be reset.
    reset_lock_screen: MethodId,
    /// `String openWallpaperSettings()`: `null`, or a short message when there is no screen.
    open_wallpaper_settings: MethodId,
}

impl AndroidWallpaper {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            lock_screen_unsupported: env.method_id(
                bridge,
                c"lockScreenUnsupported",
                c"()Ljava/lang/String;",
            )?,
            screen: env.method_id(bridge, c"screen", c"()[I")?,
            folds: env.method_id(bridge, c"folds", c"()Z")?,
            crop_image: env.method_id(bridge, c"cropImage", c"([BIIIIIIIII)[B")?,
            set_lock_screen: env.method_id(bridge, c"setLockScreen", c"([B)I")?,
            lock_screen_wallpaper_id: env.method_id(bridge, c"lockScreenWallpaperId", c"()I")?,
            reset_lock_screen: env.method_id(
                bridge,
                c"resetLockScreen",
                c"()Ljava/lang/String;",
            )?,
            open_wallpaper_settings: env.method_id(
                bridge,
                c"openWallpaperSettings",
                c"()Ljava/lang/String;",
            )?,
            bridge: env.global_ref(bridge)?,
        })
    }

    /// Calls a bridge method that answers `null` when it did what was asked, and otherwise a
    /// short message saying why not.
    fn request(&self, method: MethodId) -> Result<(), DataError> {
        let env = self.vm.env()?;
        match env.call_string(&self.bridge, method, &[])? {
            None => Ok(()),
            Some(message) => Err(DataError::Unsupported(message)),
        }
    }
}

impl WallpaperAdapter for AndroidWallpaper {
    /// `WallpaperManager.isWallpaperSupported` and `isSetWallpaperAllowed`: a device without
    /// wallpapers, or a policy that forbids changing them, says so (WALL-13).
    fn lock_screen_support(&self) -> Result<(), DataError> {
        self.request(self.lock_screen_unsupported)
    }

    /// The default display's size in its natural orientation, from its mode, so turning the
    /// device doesn't change it, with its density. Empty if the bridge can't answer.
    fn screen(&self) -> Screen {
        let answer = self
            .vm
            .env()
            .and_then(|env| env.call_ints(&self.bridge, self.screen, &[]));
        let positive = |value: i32| u32::try_from(value).unwrap_or(0);
        match answer.ok().flatten().as_deref() {
            Some(&[width, height, density_dpi]) => Screen {
                size: Size {
                    width: positive(width),
                    height: positive(height),
                },
                density_dpi: positive(density_dpi),
            },
            _ => Screen::default(),
        }
    }

    /// `PackageManager.FEATURE_SENSOR_HINGE_ANGLE`: a foldable's hinge. False if the bridge
    /// can't answer.
    fn folds(&self) -> bool {
        self.vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.folds, &[]))
            .unwrap_or(false)
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

    /// `WallpaperManager.setStream` with `FLAG_LOCK`, without backup: only the lock screen's
    /// wallpaper changes (WALL-01, SEC-05).
    fn set_lock_screen(&self, image: &[u8]) -> Result<Option<WallpaperMarker>, DataError> {
        let env = self.vm.env()?;
        let image = env.new_bytes(image)?;
        match env.call_int(
            &self.bridge,
            self.set_lock_screen,
            &[JValue::object(&image)],
        )? {
            id if id > 0 => Ok(Some(WallpaperMarker(id))),
            _ => Err(DataError::Failed(
                "The lock screen's wallpaper couldn't be set".into(),
            )),
        }
    }

    /// `WallpaperManager.getWallpaperId(FLAG_LOCK)`.
    fn lock_screen_marker(&self) -> Option<WallpaperMarker> {
        let answer = self
            .vm
            .env()
            .and_then(|env| env.call_int(&self.bridge, self.lock_screen_wallpaper_id, &[]));
        match answer {
            Ok(id) if id != UNKNOWN_ID => Some(WallpaperMarker(id)),
            _ => None,
        }
    }

    /// `WallpaperManager.clear(FLAG_LOCK)`: the lock screen no longer has a wallpaper of its
    /// own, so it shows the home wallpaper (WALL-12).
    fn reset_lock_screen(&self) -> Result<(), DataError> {
        self.request(self.reset_lock_screen)
    }

    /// The system's wallpaper picker, `ACTION_SET_WALLPAPER` (WALL-12).
    fn open_wallpaper_settings(&self) -> Result<(), DataError> {
        self.request(self.open_wallpaper_settings)
    }
}

/// The bridge couldn't decode the image, or not at the size its header gives.
fn unreadable() -> DataError {
    DataError::Invalid("The image couldn't be read".into())
}
