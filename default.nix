{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  gettext,
  glib,
  gtk4,
  libadwaita,
  cairo,
  pipewire,
  clang,
}:

rustPlatform.buildRustPackage {
  pname = "biglinux-noise-reduction-pipewire";
  version = "5.0.0";

  src = lib.cleanSource ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
    gettext
    clang
  ];

  buildInputs = [
    glib
    gtk4
    libadwaita
    cairo
    pipewire
  ];

  preBuild = ''
    install -d build-locale
    for po in po/*.po; do
      lang=$(basename "$po" .po)
      install -d "build-locale/''${lang}/LC_MESSAGES"
      msgfmt -o "build-locale/''${lang}/LC_MESSAGES/biglinux-noise-reduction-pipewire.mo" "$po"
    done
  '';

  postInstall = ''
    install -Dm644 usr/share/applications/br.com.biglinux.microphone.desktop \
      "$out/share/applications/br.com.biglinux.microphone.desktop"
    install -Dm644 usr/share/metainfo/br.com.biglinux.microphone.metainfo.xml \
      "$out/share/metainfo/br.com.biglinux.microphone.metainfo.xml"
    install -Dm644 usr/share/icons/hicolor/scalable/apps/br.com.biglinux.microphone.svg \
      "$out/share/icons/hicolor/scalable/apps/br.com.biglinux.microphone.svg"
    install -Dm644 usr/share/icons/hicolor/scalable/status/big-noise-reduction-on.svg \
      "$out/share/icons/hicolor/scalable/status/big-noise-reduction-on.svg"
    install -Dm644 usr/share/icons/hicolor/scalable/status/big-noise-reduction-off.svg \
      "$out/share/icons/hicolor/scalable/status/big-noise-reduction-off.svg"

    install -d "$out/share/biglinux-microphone/illustrations"
    install -m644 usr/share/biglinux-microphone/illustrations/*.svg \
      "$out/share/biglinux-microphone/illustrations/"

    install -Dm644 usr/share/plasma/plasmoids/org.biglinux.micnoise/metadata.json \
      "$out/share/plasma/plasmoids/org.biglinux.micnoise/metadata.json"
    install -Dm644 usr/share/plasma/plasmoids/org.biglinux.micnoise/contents/ui/main.qml \
      "$out/share/plasma/plasmoids/org.biglinux.micnoise/contents/ui/main.qml"

    for mo in build-locale/*/LC_MESSAGES/*.mo; do
      install -Dm644 "$mo" "$out/share/''${mo#build-}"
    done
  '';

  meta = with lib; {
    description = "AI-powered noise reduction for microphone and system audio (PipeWire + GTK4)";
    homepage = "https://github.com/biglinux/biglinux-noise-reduction-pipewire";
    license = licenses.gpl3Plus;
    platforms = platforms.linux;
    mainProgram = "biglinux-microphone";
  };
}
