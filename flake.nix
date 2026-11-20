{
  description = "VEYRON WM Wayland compositor and control client";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      veyron = pkgs.rustPlatform.buildRustPackage {
        pname = "veyron-wm";
        version = "0.1.0";
        src = self;

        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "--workspace" "--locked" ];
        cargoTestFlags = [ "--workspace" "--locked" ];

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.libffi pkgs.libxkbcommon pkgs.wayland ];

        installPhase = ''
          runHook preInstall
          install -Dm755 target/release/veyron "$out/bin/veyron"
          install -Dm755 target/release/veyronctl "$out/bin/veyronctl"
          runHook postInstall
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
