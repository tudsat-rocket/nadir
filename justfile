android_sdk := env_var_or_default("ANDROID_HOME", "/opt/android-sdk")
# `adb shell getprop ro.product.cpu.abilist` names what a device accepts; one ABI halves the build.
android_abis := "armeabi-v7a arm64-v8a"

default:
    @just --list

# --all-targets so test and bench targets get type-checked too, --workspace because
# default-members is ["gui"] and a bare cargo invocation only sees that crate.

check:
    cargo check --workspace --all-features --all-targets

clippy:
    cargo clippy --workspace --all-features --all-targets

test:
    cargo test --workspace --all-features

# The web build: only gui and its dependencies, since macros is a host proc-macro crate
wasm:
    cargo build -p gui --target wasm32-unknown-unknown

# The Android app, as a sideloadable debug APK. Needs cargo-ndk, an SDK and an NDK
android:
    ANDROID_HOME="{{android_sdk}}" cargo ndk {{ prepend("-t ", android_abis) }} \
        -o android/app/src/main/jniLibs build -p android --profile android
    cd android && ANDROID_HOME="{{android_sdk}}" ./gradlew assembleDebug
    adb install -r android/app/build/outputs/apk/debug/app-debug.apk

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

# Everything CI runs
suite:
    @just check
    @just test
    @just wasm
    @just fmt-check
    @just clippy
