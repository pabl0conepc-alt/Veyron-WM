{
  description = "VEYRON WM Wayland compositor and control client";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      # Libraries the compositor opens at runtime with dlopen (wayland-sys,
      # xkbcommon-dl and winit's EGL loader). They are not linked, so they must
      # be on the rpath of the installed binary and on LD_LIBRARY_PATH for the
      # headless tests and development shells.
      runtimeLibs = [ pkgs.wayland pkgs.libxkbcommon pkgs.libGL pkgs.libffi ];
      runtimeLibPath = pkgs.lib.makeLibraryPath runtimeLibs;

      veyron = pkgs.rustPlatform.buildRustPackage {
        pname = "veyron-wm";
        version = "0.1.0";
        src = self;

        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "--workspace" "--locked" ];
        cargoTestFlags = [ "--workspace" "--locked" ];

        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = runtimeLibs;

        # The headless suite drives the compositor with real Wayland clients.
        preCheck = ''
          export LD_LIBRARY_PATH="${runtimeLibPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          export XDG_RUNTIME_DIR="$(mktemp -d)"
        '';

        installPhase = ''
          runHook preInstall
          install -Dm755 target/release/veyron "$out/bin/veyron"
          install -Dm755 target/release/veyronctl "$out/bin/veyronctl"
          runHook postInstall
        '';

        postFixup = ''
          patchelf --add-rpath "${runtimeLibPath}" "$out/bin/veyron"
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

      # `nix develop`: cargo with clippy and rustfmt, and the runtime libraries
      # on LD_LIBRARY_PATH so `cargo test --workspace` runs the headless suite.
      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ veyron ];
        packages = [ pkgs.clippy pkgs.rustfmt ];
        LD_LIBRARY_PATH = runtimeLibPath;
      };
    };
}
