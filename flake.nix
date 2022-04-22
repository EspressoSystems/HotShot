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
        fenixStable = fenix.packages.${system}.stable.withComponents [ "cargo" "clippy" "rust-src" "rustc" "rustfmt" "llvm-tools-preview" ];
        # needed for compiling static binary
        fenixMusl = with fenix.packages.${system}; combine [ (stable.withComponents [ "cargo" "clippy" "rustc" "rustfmt" ]) targets.x86_64-unknown-linux-musl.stable.rust-std ];
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
                       cargo-llvm-cov = pkgs.rustPlatform.buildRustPackage rec {
               pname = "cargo-llvm-cov";
               version = "v0.3.0";

               doCheck = false;

               buildInputs = [ pkgs.libllvm ];

               src = pkgs.fetchFromGitHub {
                 owner = "DieracDelta";
                 repo = pname;
                 rev = "jr/cargo-lock";
                 sha256 = "sha256-pSM7EI+8xWihs0X8AQItSuj0GRHWW4PG9XS5/thqezI=";
               };

               cargoSha256 = "sha256-P5lxVidLQmu2PobI5S+PH8ISvFEYpU1JwNfmtLwRtzA=";
               meta = with pkgs.lib; {
                 description = "grcov collects and aggregates code coverage information for multiple source files.";
                 homepage = "https://github.com/mozilla/grcov";

                 license = licenses.mpl20;
               };
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


        # TODO uncomment when fetching dependencies is unborked
        # pkgsAndChecksList = pkgs.lib.mapAttrsToList (name: val: { packages.${name} = val.build; checks.${name} = val.build.override { runTests = true; }; }) project.workspaceMembers;
        # # programatically generate output packages based on what exists in the workspace
        # pkgsAndChecksAttrSet = pkgs.lib.foldAttrs (n: a: pkgs.lib.recursiveUpdate n a) { } pkgsAndChecksList;

        buildDeps = with pkgs; [
          cargo-audit
          nixpkgs-fmt
          git-chglog
          protobuf
          python3
          zlib.dev
          zlib.out
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
            buildInputs = with pkgs; [ flamegraph fd cargo-llvm-cov fenixStable ] ++ buildDeps;
          };
        };
        # TODO uncomment when fetching dependencies is unborked
        # packages = pkgsAndChecksAttrSet.packages;
        # checks = pkgsAndChecksAttrSet.checks;

        defaultPackage = project.workspaceMembers.phaselock.build;
      }
    );
}
