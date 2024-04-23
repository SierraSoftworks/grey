{
  description = "A lightweight, OpenTelemetry native, external synthetic probing agent.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, advisory-db, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        inherit (pkgs) lib stdenv;

        craneLib = crane.lib.${system};
        src = pkgs.nix-gitignore.gitignoreSourcePure ''
          /example
          /target
          /result
          *.nix
          '' ./.;

        nativeBuildInputs = [
          pkgs.pkg-config
        ]
        ++ lib.optionals stdenv.isDarwin [
          pkgs.libiconv
          pkgs.darwin.apple_sdk.frameworks.Security
          pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
        ];

        buildInputs = [ pkgs.openssl pkgs.protobuf ];

        cargoArtifacts = craneLib.buildDepsOnly {
          inherit src nativeBuildInputs buildInputs;
        };

        grey = craneLib.buildPackage {
          inherit cargoArtifacts src nativeBuildInputs buildInputs;

          doCheck = false;
        };
      in
      {
        checks = {
          # Build the crate as part of `nix flake check` for convenience
          inherit grey;

          # Run clippy (and deny all warnings) on the crate source,
          # again, resuing the dependency artifacts from above.
          #
          # Note that this is done as a separate derivation so that
          # we can block the CI if there are issues here, but not
          # prevent downstream consumers from building our crate by itself.
          grey-clippy = craneLib.cargoClippy {
            inherit cargoArtifacts src buildInputs;
            cargoClippyExtraArgs = "--all-targets --no-deps";
          };

          grey-doc = craneLib.cargoDoc {
            inherit cargoArtifacts src;
            cargoDocExtraArgs = "--no-deps --no-default-features";
          };

          # Check formatting
          grey-fmt = craneLib.cargoFmt {
            inherit src;
          };

          # Audit dependencies
          grey-audit = craneLib.cargoAudit {
            inherit src advisory-db;
          };

          # Run tests with cargo-nextest
          # Consider setting `doCheck = false` on `my-crate` if you do not want
          # the tests to run twice
          grey-nextest = craneLib.cargoNextest {
            inherit cargoArtifacts src nativeBuildInputs buildInputs;
            partitions = 1;
            partitionType = "count";

            # Disable impure tests (which access the network and/or filesystem)
            cargoNextestExtraArgs = "--no-fail-fast";
          };
        };

        packages.default = grey;

        apps.default = flake-utils.lib.mkApp {
          drv = grey;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = builtins.attrValues self.checks;

          # Extra inputs can be added here
          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            nodejs
          ] ++ nativeBuildInputs;
        };
      });
}