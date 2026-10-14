# Installation

Pre-built packages are attached to every [GitHub Release](https://github.com/sansob/PakPos/releases).
Each package contains both the desktop app (`pakpos`) and the CLI (`pakpos-cli`).

## Windows 10 / 11 (x64 and ARM64)

- **Installer:** run `PakPos-x.y.z-windows-x64-setup.exe` (or `-arm64-setup.exe` on
  ARM devices such as Snapdragon laptops). You can tick *Add pakpos-cli to PATH*.
- **Portable:** unzip `pakpos-x.y.z-windows-x64.zip` (or `-arm64.zip`) anywhere and run `pakpos.exe`.

Builds are not code-signed yet, so SmartScreen may warn: click **More info → Run anyway**.

## macOS 11+ (Apple Silicon and Intel)

1. Open `PakPos-x.y.z-macos-universal.dmg` and drag **PakPos** into **Applications**.
2. The app is ad-hoc signed, not notarised, so macOS blocks the first launch.
   On macOS 15 (Sequoia) and later: open PakPos once, dismiss the warning, then go to
   **System Settings → Privacy & Security**, scroll down and click **Open Anyway**.
   On older versions: **right-click PakPos → Open → Open**.
   Or, from a terminal: `xattr -dr com.apple.quarantine /Applications/PakPos.app`.

The CLI lives inside the bundle; link it onto your PATH:

```sh
sudo ln -s /Applications/PakPos.app/Contents/MacOS/pakpos-cli /usr/local/bin/pakpos-cli
```

## Linux

**Debian / Ubuntu:**

```sh
sudo apt install ./pakpos_x.y.z-1_amd64.deb   # or _arm64.deb
```

**Other distributions:** extract the tarball and copy the binaries:

```sh
tar -xzf pakpos-x.y.z-linux-x86_64.tar.gz
sudo install pakpos-x.y.z-linux-x86_64/pakpos* /usr/local/bin/
```

PakPos runs on both X11 and Wayland. It needs OpenGL 2+ (any Mesa driver works)
and `libxkbcommon`. File dialogs use the XDG desktop portal.

## Verify your download

Every release has a `SHA256SUMS.txt`:

```sh
sha256sum -c SHA256SUMS.txt --ignore-missing
```

## Build from source

You need [Rust](https://rustup.rs) 1.95 or newer.

```sh
git clone https://github.com/sansob/PakPos
cd PakPos
cargo build --release
./target/release/pakpos        # desktop app
./target/release/pakpos-cli -h # CLI
```

On Debian/Ubuntu, install the windowing headers first:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

## Where is my data?

| OS | Workspace folder |
|---|---|
| Linux | `~/.local/share/pakpos` |
| macOS | `~/Library/Application Support/dev.PakPos.PakPos` |
| Windows | `%APPDATA%\PakPos\PakPos\data` |

Set the `PAKPOS_HOME` environment variable to use any other folder, for example a
git repository shared with your team. The current path is shown in the status bar
and in **Settings**.
