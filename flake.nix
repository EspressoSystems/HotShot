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
        fenixStable = fenix.packages.${system}.stable.withComponents [ "cargo" "clippy" "rust-src" "rustc" "rustfmt" ];
        # needed for compiling static binary
        fenixMusl = with fenix.packages.${system}; combine [ (stable.withComponents [ "cargo" "clippy" "rustc" "rustfmt" ]) targets.x86_64-unknown-linux-musl.stable.rust-std ];
        # needed to generate grcov graphs
        fenixNightly = fenix.packages.${system}.latest.withComponents [ "cargo" "clippy" "rust-src" "rustc" "rustfmt" ];
        rustOverlay = final: prev:
          {
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
        crateName = "phaselock";

        inherit (import "${crate2nix}/tools.nix" { inherit pkgs; })
          generatedCargoNix;

        project = import
          (generatedCargoNix {
            name = crateName;
            src = ./.;
          })
          {
            inherit pkgs;
            # need to force threshold_crypto/use-insuecure-test-only-mock-crypto
            # otherwise a subset of tests hang
            rootFeatures = [ "demo" "docs" "blake3" "threshold_crypto/use-insecure-test-only-mock-crypto" ];
            defaultCrateOverrides = pkgs.defaultCrateOverrides // {
              # Crate dependency overrides go here
              # pass in protobuf
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
            };
          };


        pkgsAndChecksList = pkgs.lib.mapAttrsToList (name: val: { packages.${name} = val.build; checks.${name} = val.build.override { runTests = true; }; }) project.workspaceMembers;
        # programatically generate output packages based on what exists in the workspace
        pkgsAndChecksAttrSet = pkgs.lib.foldAttrs (n: a: pkgs.lib.recursiveUpdate n a) { } pkgsAndChecksList;

        buildDeps = with pkgs; [
          cargo-audit
          nixpkgs-fmt
          git-chglog
          protobuf
          python3
          fenix.packages.${system}.rust-analyzer
        ] ++ lib.optionals stdenv.isDarwin [ darwin.apple_sdk.frameworks.Security pkgs.libiconv darwin.apple_sdk.frameworks.SystemConfiguration ];

      in
      {
        devShell = pkgs.mkShell {
          buildInputs =
            with pkgs; [ fenixStable ] ++ buildDeps;
        };


        devShells = {
          # usage: compile a statically linked musl binary
          staticShell = pkgs.mkShell {
            shellHook = ''
              ulimit -n 1024
              export RUSTFLAGS='-C target-feature=+crt-static'
              export CARGO_BUILD_TARGET='x86_64-unknown-linux-musl'
            '';
            buildInputs =
              with pkgs; [ fenixMusl ] ++ buildDeps;
          };

          # usage: evaluate performance (grcov + flamegraph)
          perfShell = pkgs.mkShell {
            buildInputs = with pkgs; [ grcov flamegraph fd fenixNightly ] ++ buildDeps;
            shellHook = ''
              ulimit -n 1024
              export RUSTFLAGS='-Zprofile -Ccodegen-units=1 -Cinline-threshold=0 -Clink-dead-code -Coverflow-checks=off -Cpanic=abort -Zpanic_abort_tests'
              export RUSTDOCFLAGS='-Zprofile -Ccodegen-units=1 -Cinline-threshold=0 -Clink-dead-code -Coverflow-checks=off -Cpanic=abort -Zpanic_abort_tests'
              export CARGO_INCREMENTAL=0
            '';
          };
        };
        packages = pkgsAndChecksAttrSet.packages;
        checks = pkgsAndChecksAttrSet.checks;

        defaultPackage = project.workspaceMembers.phaselock.build;
      }
    );
}
