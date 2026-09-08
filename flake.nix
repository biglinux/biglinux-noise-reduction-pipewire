{
  description = "Filter noise — native PipeWire application and NixOS integration";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (system:
      let pkgs = import nixpkgs { inherit system; };
      in {
        packages.default = pkgs.callPackage ./default.nix { };
        packages.biglinux-noise-reduction-pipewire = self.packages.${system}.default;
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/biglinux-microphone";
        };
        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [ rustc cargo clippy rustfmt cargo-audit cargo-deny cargo-machete gettext ];
        };
      }) // {
        nixosModules.default = import ./packaging/nix/module.nix;
      };
}
