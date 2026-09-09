{ config, lib, pkgs, ... }:
let
  cfg = config.programs.biglinux-microphone;
  application = pkgs.callPackage ../../default.nix {
    inherit (cfg) denoiserPackages;
  };
in {
  options.programs.biglinux-microphone = {
    enable = lib.mkEnableOption "BigLinux microphone and playback filters";
    denoiserPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      description = ''
        Native neural LADSPA packages, including their inference runtimes.
        They must install the application's expected filenames under lib/ladspa.
        GTCRN is required by the availability probe; additional models are optional.
        This module does not download or run arbitrary third-party installers.
      '';
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [ {
      assertion = cfg.denoiserPackages != [ ];
      message = "Set programs.biglinux-microphone.denoiserPackages to your packaged GTCRN LADSPA backend and runtime.";
    } ];
    environment.systemPackages = [ application ];
    services.pipewire.enable = true;
    services.pipewire.wireplumber.enable = true;
    services.pipewire.wireplumber.configPackages = [ application ];
    systemd.packages = [ application ];
    systemd.user.services.biglinux-microphone.wantedBy = [ "default.target" ];
  };
}
