#!/usr/bin/env python3
"""Writes package-manager manifests for a PakPos release from its SHA256SUMS.

    python3 packaging/make-manifests.py --version 0.2.0 \
        --sums dist/SHA256SUMS.txt --repo sansob/PakPos --out manifests

Creates:
  homebrew/Casks/pakpos.rb          desktop app (+ pakpos-cli) for a Homebrew tap
  homebrew/Formula/pakpos-cli.rb    the CLI alone (macOS and Linux)
  winget/<id path>/<version>/*.yaml manifests for winget-pkgs
  aur/pakpos-bin/PKGBUILD, .SRCINFO the AUR package
  flatpak/io.github.sansob.PakPos.yml (+ metainfo) for flatpak-builder / Flathub

Publishing them needs your own accounts; see docs/src/contributing/packaging.md.
Only the Python standard library is used.
"""
import argparse
import json
import os
import re
import textwrap

APP_ID = "io.github.sansob.PakPos"
SUMMARY = "Native, open-source API client: HTTP, GraphQL, gRPC, WebSocket, MQTT and more"
DESCRIPTION = (
    "PakPos is a fast, native API client written in Rust. Send HTTP, GraphQL, gRPC, "
    "WebSocket, SSE, Socket.IO, MQTT and MCP requests, organize them in collections "
    "with environments and tests, run them in CI with pakpos-cli, mock APIs from "
    "saved examples and share workspaces with git."
)


def read_sums(path):
    sums = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if len(parts) == 2:
                sums[parts[1].lstrip("*")] = parts[0].lower()
    return sums


def need(sums, name):
    if name not in sums:
        raise SystemExit(f"{name} is not in SHA256SUMS (have: {', '.join(sorted(sums))})")
    return sums[name]


def write(out, rel, text):
    path = os.path.join(out, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(textwrap.dedent(text).lstrip("\n"))
    print("wrote", path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", required=True, help="e.g. 0.2.0 or 0.2.0-rc.1")
    ap.add_argument("--sums", required=True)
    ap.add_argument("--repo", default="sansob/PakPos")
    ap.add_argument("--out", default="manifests")
    a = ap.parse_args()
    v, repo, out = a.version.lstrip("v"), a.repo, a.out
    sums = read_sums(a.sums)
    owner = repo.split("/")[0]
    base = f"https://github.com/{repo}/releases/download/v{v}"
    homepage = f"https://github.com/{repo}"
    pre = "-" in v

    dmg = f"PakPos-{v}-macos-universal.dmg"
    cli_mac = f"pakpos-cli-{v}-macos-universal.tar.gz"
    lin = {arch: f"pakpos-{v}-linux-{arch}.tar.gz" for arch in ("x86_64", "aarch64")}
    win = {arch: f"PakPos-{v}-windows-{arch}-setup.exe" for arch in ("x64", "arm64")}

    # ---- Homebrew ----------------------------------------------------------------
    write(out, "homebrew/Casks/pakpos.rb", f'''
        cask "pakpos" do
          version "{v}"
          sha256 "{need(sums, dmg)}"

          url "{base.replace(v, "#{version}")}/PakPos-#{{version}}-macos-universal.dmg"
          name "PakPos"
          desc "{SUMMARY}"
          homepage "{homepage}"

          app "PakPos.app"
          binary "#{{appdir}}/PakPos.app/Contents/MacOS/pakpos-cli"

          zap trash: [
            "~/Library/Application Support/dev.pakpos.PakPos",
            "~/Library/Application Support/PakPos",
          ]

          caveats <<~EOS
            PakPos is not notarized yet. If macOS refuses to open it, run:
              xattr -dr com.apple.quarantine #{{appdir}}/PakPos.app
          EOS
        end
    ''')
    write(out, "homebrew/Formula/pakpos-cli.rb", f'''
        class PakposCli < Formula
          desc "{SUMMARY} (command line)"
          homepage "{homepage}"
          version "{v}"
          license any_of: ["MIT", "Apache-2.0"]

          on_macos do
            url "{base}/{cli_mac}"
            sha256 "{need(sums, cli_mac)}"
          end
          on_linux do
            on_intel do
              url "{base}/{lin["x86_64"]}"
              sha256 "{need(sums, lin["x86_64"])}"
            end
            on_arm do
              url "{base}/{lin["aarch64"]}"
              sha256 "{need(sums, lin["aarch64"])}"
            end
          end

          def install
            bin.install "pakpos-cli"
          end

          test do
            assert_match version.to_s, shell_output("#{{bin}}/pakpos-cli --version")
          end
        end
    ''')

    # ---- winget -------------------------------------------------------------------
    pkg_id = f"{owner}.PakPos"
    first = pkg_id[0].lower()
    wdir = f"winget/manifests/{first}/{owner}/PakPos/{v}"
    write(out, f"{wdir}/{pkg_id}.yaml", f'''
        # yaml-language-server: $schema=https://aka.ms/winget-manifest.version.1.6.0.schema.json
        PackageIdentifier: {pkg_id}
        PackageVersion: {v}
        DefaultLocale: en-US
        ManifestType: version
        ManifestVersion: 1.6.0
    ''')
    installers = "".join(
        f'''
          - Architecture: {arch}
            InstallerUrl: {base}/{win[arch]}
            InstallerSha256: {need(sums, win[arch]).upper()}'''
        for arch in ("x64", "arm64")
    )
    write(out, f"{wdir}/{pkg_id}.installer.yaml", f'''
        # yaml-language-server: $schema=https://aka.ms/winget-manifest.installer.1.6.0.schema.json
        PackageIdentifier: {pkg_id}
        PackageVersion: {v}
        InstallerType: inno
        Scope: user
        UpgradeBehavior: install
        Commands:
          - pakpos-cli
        Installers:{installers}
        ManifestType: installer
        ManifestVersion: 1.6.0
    ''')
    write(out, f"{wdir}/{pkg_id}.locale.en-US.yaml", f'''
        # yaml-language-server: $schema=https://aka.ms/winget-manifest.defaultLocale.1.6.0.schema.json
        PackageIdentifier: {pkg_id}
        PackageVersion: {v}
        PackageLocale: en-US
        Publisher: "PakPos contributors"
        PublisherUrl: {homepage}
        PackageName: PakPos
        PackageUrl: {homepage}
        License: MIT OR Apache-2.0
        LicenseUrl: {homepage}/blob/main/LICENSE-MIT
        ShortDescription: {json.dumps(SUMMARY)}
        Description: {json.dumps(DESCRIPTION)}
        Moniker: pakpos
        Tags:
          - api
          - api-client
          - http
          - rest
          - graphql
          - grpc
          - websocket
          - mqtt
        ReleaseNotesUrl: {homepage}/releases/tag/v{v}
        ManifestType: defaultLocale
        ManifestVersion: 1.6.0
    ''')

    # ---- AUR ------------------------------------------------------------------------
    # pacman versions cannot contain '-': 0.2.0-rc.1 -> 0.2.0rc.1 (sorts before 0.2.0).
    pkgver = v.replace("-", "")
    write(out, "aur/pakpos-bin/PKGBUILD", f'''
        # Maintainer: PakPos contributors <{homepage}>
        pkgname=pakpos-bin
        pkgver={pkgver}
        _ver={v}
        pkgrel=1
        pkgdesc="{SUMMARY}"
        arch=('x86_64' 'aarch64')
        url="{homepage}"
        license=('MIT' 'Apache-2.0')
        depends=('gcc-libs' 'glibc' 'libxkbcommon' 'wayland' 'libgl' 'git')
        optdepends=('xdg-utils: open links and OAuth pages in the browser')
        provides=('pakpos' 'pakpos-cli')
        conflicts=('pakpos')
        source_x86_64=("{base.replace(v, "${_ver}")}/pakpos-${{_ver}}-linux-x86_64.tar.gz")
        source_aarch64=("{base.replace(v, "${_ver}")}/pakpos-${{_ver}}-linux-aarch64.tar.gz")
        sha256sums_x86_64=('{need(sums, lin["x86_64"])}')
        sha256sums_aarch64=('{need(sums, lin["aarch64"])}')

        package() {{
          cd "pakpos-${{_ver}}-linux-${{CARCH}}"
          install -Dm755 pakpos "$pkgdir/usr/bin/pakpos"
          install -Dm755 pakpos-cli "$pkgdir/usr/bin/pakpos-cli"
          install -Dm644 pakpos.desktop "$pkgdir/usr/share/applications/pakpos.desktop"
          install -Dm644 icon-256.png "$pkgdir/usr/share/icons/hicolor/256x256/apps/pakpos.png"
          install -Dm644 LICENSE-MIT "$pkgdir/usr/share/licenses/$pkgname/LICENSE-MIT"
          install -Dm644 LICENSE-APACHE "$pkgdir/usr/share/licenses/$pkgname/LICENSE-APACHE"
        }}
    ''')
    write(out, "aur/pakpos-bin/.SRCINFO", f'''
        pkgbase = pakpos-bin
        \tpkgdesc = {SUMMARY}
        \tpkgver = {pkgver}
        \tpkgrel = 1
        \turl = {homepage}
        \tarch = x86_64
        \tarch = aarch64
        \tlicense = MIT
        \tlicense = Apache-2.0
        \tdepends = gcc-libs
        \tdepends = glibc
        \tdepends = libxkbcommon
        \tdepends = wayland
        \tdepends = libgl
        \tdepends = git
        \toptdepends = xdg-utils: open links and OAuth pages in the browser
        \tprovides = pakpos
        \tprovides = pakpos-cli
        \tconflicts = pakpos
        \tsource_x86_64 = {base}/{lin["x86_64"]}
        \tsha256sums_x86_64 = {need(sums, lin["x86_64"])}
        \tsource_aarch64 = {base}/{lin["aarch64"]}
        \tsha256sums_aarch64 = {need(sums, lin["aarch64"])}

        pkgname = pakpos-bin
    ''')

    # ---- Flatpak --------------------------------------------------------------------
    sources = "".join(
        f'''
              - type: archive
                url: {base}/{lin[arch]}
                sha256: {need(sums, lin[arch])}
                only-arches: [{arch}]'''
        for arch in ("x86_64", "aarch64")
    )
    write(out, f"flatpak/{APP_ID}.yml", f'''
        app-id: {APP_ID}
        runtime: org.freedesktop.Platform
        runtime-version: "24.08"
        sdk: org.freedesktop.Sdk
        command: pakpos
        finish-args:
          - --share=network
          - --share=ipc
          - --socket=wayland
          - --socket=fallback-x11
          - --device=dri
          # Collections, .proto files and data files live in your folders.
          - --filesystem=home
        modules:
          - name: pakpos
            buildsystem: simple
            build-commands:
              - install -Dm755 pakpos /app/bin/pakpos
              - install -Dm755 pakpos-cli /app/bin/pakpos-cli
              - install -Dm644 icon-256.png /app/share/icons/hicolor/256x256/apps/{APP_ID}.png
              - sed -e 's/^Icon=.*/Icon={APP_ID}/' pakpos.desktop > {APP_ID}.desktop
              - install -Dm644 {APP_ID}.desktop /app/share/applications/{APP_ID}.desktop
              - install -Dm644 {APP_ID}.metainfo.xml /app/share/metainfo/{APP_ID}.metainfo.xml
            sources:{sources}
              - type: file
                path: {APP_ID}.metainfo.xml
    ''')
    write(out, f"flatpak/{APP_ID}.metainfo.xml", f'''
        <?xml version="1.0" encoding="UTF-8"?>
        <component type="desktop-application">
          <id>{APP_ID}</id>
          <name>PakPos</name>
          <summary>Native, open-source API client</summary>
          <metadata_license>CC0-1.0</metadata_license>
          <project_license>MIT OR Apache-2.0</project_license>
          <developer id="io.github.{owner.lower()}">
            <name>PakPos contributors</name>
          </developer>
          <description>
            <p>{DESCRIPTION}</p>
          </description>
          <launchable type="desktop-id">{APP_ID}.desktop</launchable>
          <url type="homepage">{homepage}</url>
          <url type="bugtracker">{homepage}/issues</url>
          <screenshots>
            <screenshot type="default">
              <image>https://raw.githubusercontent.com/{repo}/main/docs/screenshots/overview.png</image>
            </screenshot>
          </screenshots>
          <content_rating type="oars-1.1"/>
          <releases>
            <release version="{v}" type="{"development" if pre else "stable"}"/>
          </releases>
        </component>
    ''')


if __name__ == "__main__":
    main()
