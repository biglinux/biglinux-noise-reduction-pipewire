{
  python3Packages,
  pipewire,
  gtk4,
  libadwaita,
  gst_all_1,
  pkg-config,
  wrapGAppsHook4,
  gobject-introspection,
}:

python3Packages.buildPythonApplication {
  pname = "biglinux-noise-reduction-pipewire";
  version = "5.0.0";

  src = ./.;

  pyproject = true;

  build-system = with python3Packages; [ setuptools ];
  dependencies = with python3Packages; [
    pygobject3
    pycairo
    numpy
  ];

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
    gobject-introspection
  ];

  buildInputs = [
    pipewire
    gtk4
    libadwaita
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
  ];

  postInstall = ''
    cp $src/usr/share $out/share -r
    cp $src/usr/bin/* $out/bin/ -r
  '';

  meta = {
    description = "AI-powered microphone noise reduction with GTK4/Libadwaita interface for PipeWire";
    homepage = "https://github.com/biglinux/biglinux-noise-reduction-pipewire";
    license = "GPL-3.0-or-later";
    mainProgram = "big-microphone-noise-reduction";
  };
}
