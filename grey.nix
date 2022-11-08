with import <nixpkgs> {};

{ lib, rustPlatform }:

rustPlatform.buildRustPackage rec {
  pname = "grey";
  version = "0.0.0-dev";

  src = nix-gitignore.gitignoreSourcePure ''
  /example
  /target
  /result
  *.nix
'' ./.;

  buildInputs = [protobuf openssl]
    ++ lib.optionals stdenv.isDarwin [darwin.apple_sdk.frameworks.Security];

  PROTOC = "${pkgs.protobuf}/bin/protoc";
  OPENSSL_DIR = "${pkgs.openssl}";

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
}