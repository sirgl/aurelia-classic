//! Aurelia (classic) on Android: the activity's entry point; the window, touch and the lifecycle are
//! `launcher_core::android`. See lands/docs/android.md (the Android build is documented once, for both launchers).
#![cfg(target_os = "android")]

use launcher_core::android::{self, AndroidApp};

/// The activity's entry point (android-activity calls it on the game thread).
#[no_mangle]
fn android_main(app: AndroidApp) {
    // What a bug report says about the build (`AURELIA_GIT_HASH`: set it when building to name the commit).
    launcher_core::client::shell::report::set_build(env!("CARGO_PKG_VERSION"), option_env!("AURELIA_GIT_HASH"));
    android::run(app, || (classic_ui::ClassicUi::game(), classic_ui::terrain_layers()));
}
