{
  description = "A lightweight, OpenTelemetry native, external synthetic probing agent.";

  outputs = { self, nixpkgs }:
    {
      devShells = nixpkgs.lib.genAttrs ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"] (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          lib = pkgs.lib;
          stdenv = pkgs.stdenv;
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
              pkgs.pkg-config
            ] ++ lib.optionals stdenv.isDarwin [
              pkgs.darwin.apple_sdk.frameworks.Security
            ];

            buildInputs = [
              pkgs.libiconv
              pkgs.openssl
            ];

            PROTOC = "${pkgs.protobuf}/bin/protoc";
          };
        }
      );

      packages = nixpkgs.lib.genAttrs ["aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux"] (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          lib = pkgs.lib;
          rustPlatform = pkgs.rustPlatform;
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
              pkgs.pkg-config
            ];

            buildInputs = [pkgs.openssl]
              ++ lib.optionals pkgs.stdenv.isDarwin [pkgs.darwin.apple_sdk.frameworks.Security pkgs.darwin.apple_sdk.frameworks.SystemConfiguration];

            PROTOC = "${pkgs.protobuf}/bin/protoc";

            cargoLock = {
              lockFile = ./Cargo.lock;

              outputHashes = {
                  "tracing-attributes-0.2.0" = "sha256-0Mm7YNgK3qkytq8eBRjUofaLLOVt/p5NF+jeBLr/K/Y=";
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
