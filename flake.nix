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
            name = "grey_dev_shell";
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
              pkgs.openssl
              pkgs.libiconv
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
          nodePackages = pkgs.nodePackages;
        in
        {
          grey = rustPlatform.buildRustPackage rec {
            pname = "grey";
            system = system;

            src = pkgs.nix-gitignore.gitignoreSourcePure ''
            /example
            /target
            /result
            *.nix
          '' ./.;

            buildInputs = [pkgs.protobuf pkgs.openssl pkgs.pkg-config]
              ++ lib.optionals pkgs.stdenv.isDarwin [pkgs.darwin.apple_sdk.frameworks.Security];

            PROTOC = "${pkgs.protobuf}/bin/protoc";

            cargoLock = {
              lockFile = ./Cargo.lock;

              outputHashes = {
                  "tracing-0.2.0" = "sha256-xK2F6TNne+scfKgU4Vr1tfe0kdXyOZt0N7bex0Jzcmg=";
              };
            };

            meta = with lib; {
              description = "A lightweight, OpenTelemetry native, external synthetic probing agent.";
              homepage = "https://github.com/SierraSoftworks/grey";
              license = licenses.mit;
              maintainers = [ maintainers.tailhook ];
            };
          };
          default = self.packages.${system}.grey;
        }
      );
    };
}
