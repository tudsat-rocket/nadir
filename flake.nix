{
  description = "rapid-control";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
  };
  outputs =
    {
      rust-overlay,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system}.extend (import rust-overlay);
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        nativeBuildInputs = with pkgs; [
          # for gui
          pkg-config
          wrapGAppsHook3
        ];
        buildInputs =
          with pkgs;
          [
            # for gui
            glib
            libudev-zero
            gtk3
            atkmm
            # for rfd
            zenity
            xdg-desktop-portal
            # for winit/wayland+x11 (may not all be needed, I guessed)
            wayland
            libxkbcommon
            libGL
            libx11
            libxcursor
            libxrandr
            libxi
          ]
          ++ [ rustToolchain ];
        LD_LIBRARY_PATH =
          with pkgs;
          nixpkgs.lib.makeLibraryPath [
            # for gubi
            libGL
            libxkbcommon
            # (may not all be needed, I guessed)
            wayland
            libx11
            libxcursor
            libxrandr
            libxi
          ];

        rapid-control = pkgs.rustPlatform.buildRustPackage {
          pname = "rapid-control";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };
          inherit nativeBuildInputs buildInputs LD_LIBRARY_PATH;

        };
      in
      {
        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs buildInputs LD_LIBRARY_PATH;
          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.dbus.lib}/lib:$LD_LIBRARY_PATH"
          '';
        };

        formatter = pkgs.nixfmt-rfc-style;

        packages = {
          default = rapid-control;
        };
      }
    );
}
