# Build ChromeThemer

Run these commands from the repo root. The frontend uses Node.js 22 in CI, and the Rust workspace needs a stable toolchain with Rust 2024 support.

```sh
npm ci
```

## Windows x64

The local Windows build uses MSYS2 UCRT64 GCC and GNU Rust.

Install GCC from the MSYS2 UCRT64 terminal:

```sh
pacman -S --needed mingw-w64-ucrt-x86_64-gcc
```

Install the Rust toolchain, then build from PowerShell:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
$env:Path = 'C:\msys64\ucrt64\bin;' + $env:Path
$env:CHROMETHEMER_FRAME_CC = 'C:\msys64\ucrt64\bin\gcc.exe'
npm run build
cargo +stable-x86_64-pc-windows-gnu build --release -p chromethemer-desktop --features custom-protocol
```

If MSYS2 isn't in `C:\msys64`, change both paths above.

To package the executable and DLLs:

```powershell
Copy-Item -LiteralPath target/release/chromethemer-desktop.exe -Destination target/release/ChromeThemer.exe
Compress-Archive -LiteralPath target/release/ChromeThemer.exe, target/release/WebView2Loader.dll, target/release/ChromeThemerFrame.dll -DestinationPath target/release/ChromeThemer-windows-x64.zip -Force
```

Extract these files into the same folder:

```text
ChromeThemer.exe
WebView2Loader.dll
ChromeThemerFrame.dll
```

The packaged GUI embeds the frontend. It doesn't need a localhost server. Keep both DLLs beside the executable.

## Linux

On Ubuntu, install the dependencies used by CI:

```sh
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential libssl-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev patchelf x11-utils file
```

Build the Debian package and AppImage:

```sh
npm run tauri -- build --bundles deb,appimage --features custom-protocol
```

Packages go under `target/release/bundle/`. For a build with an explicit `--target`, they're under `target/<target>/release/bundle/` instead.

## macOS

Install the Xcode command-line tools and a stable Rust toolchain, then build the app and DMG:

```sh
xcode-select --install
npm run tauri -- build --bundles app,dmg --features custom-protocol
```

The app goes in `target/release/bundle/macos/` and the DMG in `target/release/bundle/dmg/`. These commands don't configure release signing or notarization.

## CLI only

The CLI doesn't need the frontend build:

```sh
cargo build --release -p chromethemer
```

On Windows, put MSYS2 UCRT64 GCC on PATH and use the GNU toolchain:

```powershell
$env:Path = 'C:\msys64\ucrt64\bin;' + $env:Path
cargo +stable-x86_64-pc-windows-gnu build --release -p chromethemer
```

The output is `target/release/chromethemer`, or `chromethemer.exe` on Windows. The CLI takes a PAK path and doesn't apply `colors.json`.

## Release packages

The [Build packages workflow](../.github/workflows/build.yml) builds x64 and ARM64 packages for Windows, macOS, and Linux. Windows x64 uses MSYS2 GCC and GNU Rust. Windows ARM64 uses MSVC Rust with MSYS2 CLANGARM64 for the frame DLL. The local Windows commands above only build x64.

Run the workflow manually with `release_tag` left empty to download the results as Actions artifacts. It produces Windows installers and portable ZIPs, macOS apps and DMGs, and Linux Debian packages and AppImages. Local commands above don't reproduce all of the workflow's packaging checks.
