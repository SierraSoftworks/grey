{
  description = "A lightweight, OpenTelemetry native, external synthetic probing agent.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        inherit (pkgs) lib;

        rustToolchainFor =
          p:
          p.rust-bin.stable.latest.default.override {
            # Set the build targets supported by the toolchain,
            # wasm32-unknown-unknown is required for trunk.
            targets = [ "wasm32-unknown-unknown" ];
          };
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchainFor;

        # When filtering sources, we want to allow assets other than .rs files
        unfilteredRoot = ./.; # The original, unfiltered source
        src = lib.fileset.toSource {
          root = unfilteredRoot;
          fileset = lib.fileset.unions [
            # Default files from crane (Rust and cargo files)
            (craneLib.fileset.commonCargoSources unfilteredRoot)
            (lib.fileset.fileFilter (
              file:
              lib.any file.hasExt [
                "html"
                "css"
                "scss"
              ]
            ) unfilteredRoot)
            # Example of a folder for images, icons, etc
            (lib.fileset.maybeMissing ./public)
          ];
        };

        # Arguments to be used by both the client and the server
        # When building a workspace with crane, it's a good idea
        # to set "pname" and "version".
        commonArgs = {
          inherit src;
          strictDeps = true;

          buildInputs = [
            # Add additional build inputs here
            pkgs.pkg-config
            pkgs.openssl
            pkgs.protobuf
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            # Additional darwin specific inputs can be set here
            pkgs.libiconv
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];

          OPENSSL_DIR = pkgs.openssl;
          OPENSSL_INCLUDE_DIR = pkgs.openssl.dev.include;
        };

        # Native packages

        nativeArgs = commonArgs // {
          pname = "grey";
        };

        # Build *just* the cargo dependencies, so we can reuse
        # all of that work (e.g. via cachix) when running in CI
        cargoArtifacts = craneLib.buildDepsOnly nativeArgs;

        # Simple JSON API that can be queried by the client
        grey = craneLib.buildPackage (
          nativeArgs
          // {
            inherit cargoArtifacts;
            # The server needs to know where the client's dist dir is to
            # serve it, so we pass it as an environment variable at build time
            CLIENT_DIST = grey-ui;
            preBuild = ''
              cp -r $CLIENT_DIST ./ui/dist
            '';
            doCheck = false;
          }
        );

        # Wasm packages

        # it's not possible to build the server on the
        # wasm32 target, so we only build the client.
        wasmArgs = commonArgs // {
          pname = "grey-ui";
          cargoExtraArgs = "--package=grey-ui --features=wasm";
          CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
        };

        cargoArtifactsWasm = craneLib.buildDepsOnly (
          wasmArgs
          // {
            doCheck = false;
          }
        );

        # Build the frontend of the application.
        # This derivation is a directory you can put on a webserver.
        grey-ui = craneLib.buildTrunkPackage (
          wasmArgs
          // {
            pname = "grey-ui";
            cargoArtifacts = cargoArtifactsWasm;
            trunkIndexPath = "./ui/index.html";
            # The version of wasm-bindgen-cli here must match the one from Cargo.lock.
            # When updating to a new version replace the hash values with lib.fakeHash,
            # then try to do a build, which will fail but will print out the correct value
            # for `hash`. Replace the value and then repeat the process but this time the
            # printed value will be for the second `hash` below
            wasm-bindgen-cli = pkgs.buildWasmBindgenCli rec {
              src = pkgs.fetchCrate {
                pname = "wasm-bindgen-cli";
                version = "0.2.100";
                hash = "sha256-3RJzK7mkYFrs7C/WkhW9Rr4LdP5ofb2FdYGz1P7Uxog=";
                # hash = lib.fakeHash;
              };

              cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
                inherit src;
                inherit (src) pname version;
                hash = "sha256-qsO12332HSjWCVKtf1cUePWWb9IdYUmT+8OPj/XP2WE=";
                # hash = lib.fakeHash;
              };
            };
          }
        );
      in
      {
        checks = {
          # Build the crate as part of `nix flake check` for convenience
          inherit grey grey-ui;

          # Run clippy (and deny all warnings) on the crate source,
          # again, reusing the dependency artifacts from above.
          #
          # Note that this is done as a separate derivation so that
          # we can block the CI if there are issues here, but not
          # prevent downstream consumers from building our crate by itself.
          grey-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );

          # Check formatting
          grey-fmt = craneLib.cargoFmt commonArgs;
        };

        apps.default = flake-utils.lib.mkApp {
          name = "grey";
          drv = grey;
        };

        packages.default = grey;

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks.
          checks = self.checks.${system};

          # Extra inputs can be added here; cargo and rustc are provided by default.
          packages = [
            pkgs.trunk
            pkgs.nodejs
          ];
        };
      }
    );
}
