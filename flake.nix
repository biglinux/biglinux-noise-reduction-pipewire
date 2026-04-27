{
  description = "biglinux-noise-reduction-pipewire — AI noise reduction for PipeWire (GTK4 + Plasma applet)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in {
        packages = rec {
          default = biglinux-noise-reduction-pipewire;
          biglinux-noise-reduction-pipewire = pkgs.callPackage ./default.nix { };
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/biglinux-microphone";
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [
            rustc
            cargo
            clippy
            rustfmt
            cargo-outdated
            cargo-audit
            cargo-deny
            cargo-machete
            gettext
          ];
          shellHook = ''
            echo "biglinux-noise-reduction-pipewire dev shell — cargo build --release"
          '';
        };
      });
}
