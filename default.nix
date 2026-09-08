{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gettext,
  python3,
  glib,
  gtk4,
  libadwaita,
  cairo,
  pipewire,
  wireplumber,
  systemd,
  ladspaPlugins,
  symlinkJoin,
  denoiserPackages ? [ ],
}:
let
  plugins = symlinkJoin {
    name = "biglinux-microphone-ladspa";
    paths = [ ladspaPlugins ] ++ denoiserPackages;
  };
in
rustPlatform.buildRustPackage {
  pname = "biglinux-noise-reduction-pipewire";
  version = "5.0.0";
  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;

  # The BigLinux allocator patch is not a stock Nixpkgs jemalloc dependency.
  # Use the supported system-allocator build instead of linking an undeclared
  # or unverified replacement. Keep build and test features identical.
  buildNoDefaultFeatures = true;
  checkNoDefaultFeatures = true;

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
    gettext
    python3
    rustPlatform.bindgenHook
  ];
  buildInputs = [ glib gtk4 libadwaita cairo pipewire ];

  # Native distro packages use /usr; Nix must reference its own closure.
  # Patch code and its contract fixtures together, never the user's files.
  postPatch = ''
    export BIGMIC_OUT="$out"
    ${python3}/bin/python3 - <<'PY'
    import os
    from pathlib import Path
    out = os.environ["BIGMIC_OUT"]
    replacements = {
        "/usr/bin/biglinux-microphone": out + "/bin/biglinux-microphone",
        "/usr/bin/pw-": "${lib.getBin pipewire}/bin/pw-",
        "/usr/bin/wpctl": "${lib.getBin wireplumber}/bin/wpctl",
        "/usr/bin/systemctl": "${lib.getBin systemd}/bin/systemctl",
        "/usr/bin/journalctl": "${lib.getBin systemd}/bin/journalctl",
        "/usr/lib/ladspa": "${plugins}/lib/ladspa",
        "/usr/share/locale": out + "/share/locale",
        "/usr/share/biglinux-microphone": out + "/share/biglinux-microphone",
    }
    for directory in ["src", "tests", "usr"]:
        for path in Path(directory).rglob("*"):
            if not path.is_file() or path.suffix not in [".rs", ".qml", ".service", ".desktop"]:
                continue
            text = path.read_text()
            for old, new in replacements.items():
                text = text.replace(old, new)
            path.write_text(text)
    PY
  '';

  preBuild = ''
    install -d build-locale
    for po in po/*.po; do
      lang=$(basename "$po" .po)
      install -d "build-locale/''${lang}/LC_MESSAGES"
      msgfmt --check --check-header -o "build-locale/''${lang}/LC_MESSAGES/biglinux-microphone.mo" "$po"
    done
  '';

  postInstall = ''
    install -Dm644 usr/share/applications/br.com.biglinux.microphone.desktop \
      "$out/share/applications/br.com.biglinux.microphone.desktop"
    install -Dm644 usr/share/metainfo/br.com.biglinux.microphone.metainfo.xml \
      "$out/share/metainfo/br.com.biglinux.microphone.metainfo.xml"
    for category in apps status; do
      install -d "$out/share/icons/hicolor/scalable/$category"
      install -m644 usr/share/icons/hicolor/scalable/"$category"/*.svg \
        "$out/share/icons/hicolor/scalable/$category/"
    done
    install -d "$out/share/biglinux-microphone/illustrations"
    install -m644 usr/share/biglinux-microphone/illustrations/*.svg \
      "$out/share/biglinux-microphone/illustrations/"
    install -d "$out/share/plasma/plasmoids"
    cp -r usr/share/plasma/plasmoids/br.com.biglinux.micnoise "$out/share/plasma/plasmoids/"
    install -d "$out/lib/systemd/user"
    install -m644 usr/lib/systemd/user/*.service "$out/lib/systemd/user/"
    install -d "$out/share/wireplumber"
    cp -r usr/share/wireplumber/. "$out/share/wireplumber/"
    for mo in build-locale/*/LC_MESSAGES/*.mo; do
      install -Dm644 "$mo" "$out/share/''${mo#build-}"
    done
    install -Dm644 LICENSE "$out/share/licenses/biglinux-microphone/LICENSE"
    install -Dm644 LICENSE-MIT "$out/share/licenses/biglinux-microphone/LICENSE-MIT"
  '';

  passthru.requiredLadspaPackages = [ plugins ];
  meta = {
    description = "Noise reduction for microphone and playback with PipeWire";
    homepage = "https://github.com/biglinux/biglinux-noise-reduction-pipewire";
    license = lib.licenses.gpl3Plus;
    platforms = [ "x86_64-linux" ];
    mainProgram = "biglinux-microphone";
  };
}
