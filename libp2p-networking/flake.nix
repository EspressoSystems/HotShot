{
  description = "PhaseLock consensus library";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
    crate2nix = {
      url = "github:balsoft/crate2nix/balsoft/fix-broken-ifd";
      flake = false;
    };
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-compat, utils, crate2nix, fenix }:
    utils.lib.eachDefaultSystem (system:
      let
        fenixStable = with fenix.packages.${system}; combine [ (stable.withComponents [ "cargo" "clippy" "rustc" "rustfmt" ]) targets.x86_64-unknown-linux-musl.stable.rust-std];
        fenixNightly = fenix.packages.${system}.latest.withComponents [ "cargo" "clippy" "rust-src" "rustc" "rustfmt" ];
        rustOverlay = final: prev:
          {
            rustcNightly = fenixNightly;
            cargoNightly = fenixNightly;
            rust-srcNightly = fenixNightly;
            rustc = fenixStable;
            cargo = fenixStable;
            rust-src = fenixStable;
          };

        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            rustOverlay
          ];
        };

        # DON'T FORGET TO PUT YOUR PACKAGE NAME HERE, REMOVING `throw`
        crateName = "networking-demo";

        inherit (import "${crate2nix}/tools.nix" { inherit pkgs; })
          generatedCargoNix;

        project = import
          (generatedCargoNix {
            name = crateName;
            src = ./.;
          })
          {
            inherit pkgs;
            defaultCrateOverrides = pkgs.defaultCrateOverrides // {
              # Crate dependency overrides go here
              prost-build = attrs: {
                buildInputs = [ pkgs.protobuf ];
                PROTOC = "${pkgs.protobuf}/bin/protoc";
                PROTOC_INCLUDE = "${pkgs.protobuf}/include";
              };
              libp2p-core = attrs: {
                buildInputs = [ pkgs.protobuf ];
                PROTOC = "${pkgs.protobuf}/bin/protoc";
                PROTOC_INCLUDE = "${pkgs.protobuf}/include";
              };
              networking-demo = attrs: {
                buildInputs = (attrs.buildInputs or [ ]) ++ (pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.darwin.apple_sdk.frameworks.Security ]);
              };
            };
          };

        recent_flamegraph = pkgs.cargo-flamegraph.overrideAttrs (oldAttrs: rec {
          src = pkgs.fetchFromGitHub {
            owner = "flamegraph-rs";
            repo = "flamegraph";
            rev = "a9837efd8744dac853acbe3d476924083f26743d";
            sha256 = "sha256-Fek2AJ8mYYf92NBT0pmVBqK1BXmr6GdEqt2Rp8To8tI=";
          };
          version = "master";
          cargoDeps = oldAttrs.cargoDeps.overrideAttrs (pkgs.lib.const {
              name = "${oldAttrs.pname}-vendor.tar.gz";
              inherit src;
              outputHash = "sha256-i9/Fe+JaCcdHw2CR0n4hULokDN2RvDAzgXNG2zAUFDg=";
          });
        });
        # https://github.com/NixOS/nixpkgs/issues/149209
        grcov = pkgs.rustPlatform.buildRustPackage rec {
          pname = "grcov";
          version = "v0.8.7";

          doCheck = false;

          src = pkgs.fetchFromGitHub {
            owner = "mozilla";
            repo = pname;
            rev = version;
            sha256 = "sha256-4McF9tLIjDCftyGI29pm/LnTUBVWG+pY5z+mGFKySQM=";
          };

          cargoSha256 = "sha256-CdsTAMiEK8udRoD9PQSC0jQbPkdynFL0Tdw5aAZUmsM=";

          meta = with pkgs.lib; {
            description = "grcov collects and aggregates code coverage information for multiple source files.";
            homepage = "https://github.com/mozilla/grcov";
            license = licenses.mpl20;
          };
        };

        buildDeps = with pkgs; [
          cargo-audit nixpkgs-fmt git-chglog protobuf
        ] ++ lib.optionals stdenv.isDarwin [ darwin.apple_sdk.frameworks.Security pkgs.libiconv darwin.apple_sdk.frameworks.SystemConfiguration];

      in
      {
        packages.${crateName} = project.rootCrate.build;
        checks.${crateName} = project.rootCrate.build.override {
          runTests = true;
        };

        defaultPackage = self.packages.${system}.${crateName};

        devShells.staticShell = pkgs.mkShell {
          shellHook = ''
            ulimit -n 1024
            export RUSTFLAGS='-C target-feature=+crt-static'
            export CARGO_BUILD_TARGET='x86_64-unknown-linux-musl'
          '';
          buildInputs =
            with pkgs; [ fenix.packages.${system}.rust-analyzer fenixStable ] ++ buildDeps;
        };

        devShell = pkgs.mkShell {
          buildInputs =
            with pkgs; [ fenix.packages.${system}.rust-analyzer fenixStable ] ++ buildDeps;
        };

        devShells.perfShell = pkgs.mkShell {
          buildInputs = with pkgs; [ grcov recent_flamegraph fd fenixNightly fenix.packages.${system}.rust-analyzer ] ++ buildDeps;
          shellHook = ''
            ulimit -n 1024
            export RUSTFLAGS='-Zprofile -Ccodegen-units=1 -Cinline-threshold=0 -Clink-dead-code -Coverflow-checks=off -Cpanic=abort -Zpanic_abort_tests'
            export RUSTDOCFLAGS='-Zprofile -Ccodegen-units=1 -Cinline-threshold=0 -Clink-dead-code -Coverflow-checks=off -Cpanic=abort -Zpanic_abort_tests'
            export CARGO_INCREMENTAL=0
          '';
        };

      });
}
