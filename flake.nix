{
  description = "VEYRON WM Wayland compositor and control client";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      runtimeLibs = [ pkgs.libffi pkgs.libxkbcommon pkgs.wayland ];
      veyron = pkgs.rustPlatform.buildRustPackage {
        pname = "veyron-wm";
        version = "0.1.0";
        src = self;

        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "--workspace" "--locked" ];
        cargoTestFlags = [ "--workspace" "--locked" ];

        nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper ];
        buildInputs = runtimeLibs;

        # wayland-client uses dlopen, so Nix's ELF RPATH patching alone is not
        # enough for the headless integration tests to find libwayland-client.
        preCheck = ''
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        '';

        postInstall = ''
          wrapProgram "$out/bin/veyron" \
            --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}"
          wrapProgram "$out/bin/veyronctl" \
            --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}"
        '';

        meta = {
          description = "A lightweight, scrollable tiling Wayland compositor";
          homepage = "https://github.com/pabl0conepc-alt/Veyron-WM";
          license = pkgs.lib.licenses.mit;
          mainProgram = "veyron";
          platforms = [ system ];
        };
      };
    in {
      packages.${system} = {
        default = veyron;
        veyron-wm = veyron;
      };
    };
}
