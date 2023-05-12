{
  description = "A lightweight, OpenTelemetry native, external synthetic probing agent.";

  outputs = { self, nixpkgs }:
    {
      devShells = nixpkgs.lib.genAttrs ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"] (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          lib = pkgs.lib;
          stdenv = pkgs.stdenv;
          librusty_v8 = pkgs.callPackage ./librusty_v8.nix { };
          libtcc = pkgs.tinycc.overrideAttrs (oa: {
            makeFlags = [ "libtcc.a" ];
            # tests want tcc binary
            doCheck = false;
            outputs = [ "out" ];
            installPhase = ''
              mkdir -p $out/lib/
              mv libtcc.a $out/lib/
            '';
            # building the whole of tcc on darwin is broken in nixpkgs
            # but just building libtcc.a works fine so mark this as unbroken
            meta.broken = false;
          });
        in
        {
          default = stdenv.mkDerivation {
            name = "grey-shell-${system}";
            system = system;
            nativeBuildInputs = [
              pkgs.rustc
              pkgs.cargo
              pkgs.nodejs
              pkgs.protobuf
              pkgs.tinycc
              pkgs.pkg-config
            ] ++ lib.optionals stdenv.isDarwin [
              pkgs.darwin.apple_sdk.frameworks.Security
            ];

            buildInputs = [
              pkgs.libiconv
              pkgs.openssl
            ];

            PROTOC = "${pkgs.protobuf}/bin/protoc";

            # The v8 package will try to download a `librusty_v8.a` release at build time to our read-only filesystem
            # To avoid this we pre-download the file and export it via RUSTY_V8_ARCHIVE
            RUSTY_V8_ARCHIVE = librusty_v8;

            # The deno_ffi package currently needs libtcc.a on linux and macos and will try to compile it at build time
            # To avoid this we point it to our copy (dir)
            # In the future tinycc will be replaced with asm
            TCC_PATH = "${libtcc}/lib";
          };
        }
      );

      packages = nixpkgs.lib.genAttrs ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"] (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          lib = pkgs.lib;
          rustPlatform = pkgs.rustPlatform;
          librusty_v8 = pkgs.callPackage ./librusty_v8.nix { };
          libtcc = pkgs.tinycc.overrideAttrs (oa: {
            makeFlags = [ "libtcc.a" ];
            # tests want tcc binary
            doCheck = false;
            outputs = [ "out" ];
            installPhase = ''
              mkdir -p $out/lib/
              mv libtcc.a $out/lib/
            '';
            # building the whole of tcc on darwin is broken in nixpkgs
            # but just building libtcc.a works fine so mark this as unbroken
            meta.broken = false;
          });
        in
        {
          grey = rustPlatform.buildRustPackage rec {
            name = "grey-${system}";
            pname = "grey";

            src = pkgs.nix-gitignore.gitignoreSourcePure ''
            /example
            /target
            /result
            *.nix
            '' ./.;

            doCheck = false;

            nativeBuildInputs = [
              pkgs.protobuf
              pkgs.tinycc
              pkgs.pkg-config
            ];

            buildInputs = [pkgs.openssl]
              ++ lib.optionals pkgs.stdenv.isDarwin [pkgs.darwin.apple_sdk.frameworks.Security];

            PROTOC = "${pkgs.protobuf}/bin/protoc";

            # The v8 package will try to download a `librusty_v8.a` release at build time to our read-only filesystem
            # To avoid this we pre-download the file and export it via RUSTY_V8_ARCHIVE
            RUSTY_V8_ARCHIVE = librusty_v8;

            # The deno_ffi package currently needs libtcc.a on linux and macos and will try to compile it at build time
            # To avoid this we point it to our copy (dir)
            # In the future tinycc will be replaced with asm
            TCC_PATH = "${libtcc}/lib";

            cargoLock = {
              lockFile = ./Cargo.lock;

              outputHashes = {
                  "tracing-0.2.0" = "sha256-AWRx6P6slEusJbDrxMSxAkz4biQZH4iqVrdV09Olnc8=";
              };
            };

            meta = with lib; {
              description = "A lightweight, OpenTelemetry native, external synthetic probing agent.";
              homepage = "https://github.com/SierraSoftworks/grey";
              license = licenses.mit;
              maintainers = [
                {
                  name = "Benjamin Pannell";
                  email = "contact@sierrasoftworks.com";
                }
              ];
            };
          };
          default = self.packages.${system}.grey;
        }
      );
    };
}
