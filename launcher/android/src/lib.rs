//! Aurelia (classic) on Android: the activity's entry point; the window, touch and the lifecycle are
//! `launcher_core::android`. See lands/docs/android.md (the Android build is documented once, for both launchers).
#![cfg(target_os = "android")]

use launcher_core::android::{self, AndroidApp};

/// The activity's entry point (android-activity calls it on the game thread).
#[no_mangle]
fn android_main(app: AndroidApp) {
    android::run(app, || (classic_ui::ClassicUi::game(), classic_ui::terrain_layers()));
}
