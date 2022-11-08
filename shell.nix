let
  moz_overlay = import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz);
  nixpkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
in
  with nixpkgs;
  stdenv.mkDerivation {
    name = "grey_shell";
    nativeBuildInputs = [
      nixpkgs.latest.rustChannels.nightly.rust
      nixpkgs.libiconv
      nixpkgs.nodejs
      nixpkgs.protobuf
      nixpkgs.pkg-config
    ]
    ++ lib.optionals stdenv.isDarwin [darwin.apple_sdk.frameworks.Security];

    buildInputs = [
      nixpkgs.openssl
    ];

    PROTOC = "${pkgs.protobuf}/bin/protoc";
  }