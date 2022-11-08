let
  moz_overlay = import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz);
  nixpkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
in
  with nixpkgs;
  stdenv.mkDerivation {
    name = "grey_shell";
    buildInputs = [
      # to use the latest nightly:
      nixpkgs.latest.rustChannels.nightly.rust
      nixpkgs.libiconv
      nixpkgs.nodejs
      nixpkgs.openssl
      nixpkgs.protobuf
    ]
    ++ lib.optionals stdenv.isDarwin [darwin.apple_sdk.frameworks.Security];

    PROTOC = "${pkgs.protobuf}/bin/protoc";
    OPENSSL_DIR = "${pkgs.openssl.dev}";
  }