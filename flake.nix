{
  description = "HotShot consensus library";

  nixConfig = {
    extra-substituters = [ "https://espresso-systems-private.cachix.org" ];
    extra-trusted-public-keys = [
      "espresso-systems-private.cachix.org-1:LHYk03zKQCeZ4dvg3NctyCq88e44oBZVug5LpYKjPRI="
    ];
  };

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
    cargo-careful = {
      url = "github:RalfJung/cargo-careful";
      flake = false;
    };
  };

  outputs =
    { self, nixpkgs, flake-compat, utils, crate2nix, fenix, cargo-careful }:
    utils.lib.eachDefaultSystem (system:
      let
        fenixNightly = fenix.packages.${system}.latest.withComponents [
          "cargo"
          "clippy"
          "rust-src"
          "rustc"
          "rustfmt"
          "llvm-tools-preview"
        ];
        fenixStable = fenix.packages.${system}.combine [
          (fenix.packages.${system}.latest.withComponents [
            "rustfmt"
          ])
          (fenix.packages.${system}.fromToolchainFile {
            dir = ./.;
            sha256 = "sha256-yMuSb5eQPO/bHv+Bcf/US8LVMbf/G/0MSfiPwBhiPpk=";
          })
        ];
        # needed for compiling static binary
        fenixMusl = with fenix.packages.${system};
          combine [
            (stable.withComponents [ "cargo" "clippy" "rustc" "rustfmt" ])
            targets.x86_64-unknown-linux-musl.stable.rust-std
          ];

        CARGO_TARGET_DIR = "target_dirs/nix_rustc";

        pkgs = import nixpkgs {
          inherit system;
        };

        pkgsAllowUnfree = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        heapstack_pkgs = import nixpkgs { inherit system; };

        tokio-console = pkgs.rustPlatform.buildRustPackage rec {
          pname = "tokio-console";
          version = "0.1.7";

          src = pkgs.fetchFromGitHub {
            owner = "tokio-rs";
            repo = "console";
            rev = "tokio-console-v${version}";
            sha256 = "sha256-yTNLKpBkzzN0X73CjN/UXRGjAGOnCCgJa6A6loA6baM=";
          };

          cargoSha256 = "sha256-K/auhqlL/K6RYE0lHyvSUqK1cOwJBBZD3QTUevZzLXQ=";

          nativeBuildInputs = [ pkgs.protobuf ];

          meta = with pkgs.lib; {
            description = "A debugger for asynchronous Rust code";
            homepage = "https://github.com/tokio-rs/console";
            license = with licenses; [ mit ];
            maintainers = with maintainers; [ max-niederman ];
          };

          doCheck = false;
        };

        careful = pkgs.rustPlatform.buildRustPackage {
          pname = "cargo-careful";
          version = "master";

          src = cargo-careful;

          cargoSha256 = "sha256-5H6Dp3YANVGYxvphTdnd92+0h0ddFX7u5SWT24YxzV4=";

          meta = {
            description = "A cargo undefined behaviour checker";
            homepage = "https://github.com/RalfJung/cargo-careful";
          };
        };

        cargo-llvm-cov = pkgs.rustPlatform.buildRustPackage rec {
          pname = "cargo-llvm-cov";
          version = "0.3.0";

          doCheck = false;

          buildInputs = [ pkgs.libllvm ];

          src = builtins.fetchTarball {
            url =
              "https://crates.io/api/v1/crates/${pname}/${version}/download";
            sha256 =
              "sha256:0iswa2cdaf2123vfc42yj9l8jx53k5jm2y51d4xqc1672hi4620l";
          };

          cargoSha256 = "sha256-RzIkW/eytU8ZdZ18x0sGriJ2xvjVW+8hB85In12dXMg=";
          meta = {
            description = "Cargo llvm cov generates code coverage via llvm.";
            homepage = "https://github.com/taiki-e/cargo-llvm-cov";
          };
        };

        # DON'T FORGET TO PUT YOUR PACKAGE NAME HERE, REMOVING `throw`
        crateName = "hotshot";

        inherit (import "${crate2nix}/tools.nix" { inherit pkgs; })
          generatedCargoNix;

        project = import (generatedCargoNix {
          name = crateName;
          src = ./.;
        }) {
          inherit pkgs;
          rootFeatures = [ "docs" "blake3" ];
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
        # # programmatically generate output packages based on what exists in the workspace
        # pkgsAndChecksAttrSet = pkgs.lib.foldAttrs (n: a: pkgs.lib.recursiveUpdate n a) { } pkgsAndChecksList;

        buildDepsSimple = with pkgs;
          [
            curl.out
            cargo-workspaces
            cargo-audit
            cargo-nextest
            cargo-hakari
            nixpkgs-fmt
            git-chglog
            typos
            protobuf
            python3
            zlib.dev
            zlib.out
            just
            pkg-config
            openssl.dev
            openssl.out
          ] ++ lib.optionals stdenv.isDarwin [
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.CoreServices
            pkgs.libiconv
            darwin.apple_sdk.frameworks.SystemConfiguration
          ];

        buildDeps = buildDepsSimple ++ [ fenix.packages.${system}.rust-analyzer ];
      in {
        devShell = pkgs.mkShell {
          inherit CARGO_TARGET_DIR;
          buildInputs = [ fenixStable ] ++ buildDeps;
        };

        devShells = {
          # A simple shell without rust-analyzer
          simpleShell = pkgs.mkShell {
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
            '';
            buildInputs = [ fenixStable ] ++ buildDepsSimple;
          };

          # usage: check correctness
          correctnessShell = pkgs.mkShell {
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
              ulimit -n 1024
            '';
            RUST_SRC_PATH = "${fenixNightly}/lib/rustlib/src/rust/library";
            RUST_LIB_SRC = "${fenixNightly}/lib/rustlib/src/rust/library";
            buildInputs = [ careful pkgs.git fenixNightly pkgs.cargo-udeps ] ++ buildDeps;

          };

          semverShell = pkgs.mkShell {
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
            '';
            buildInputs = [
              (pkgs.cargo-semver-checks.overrideAttrs (final: prev: {doCheck = false;}))
              fenixStable
            ] ++ buildDeps;
          };

          # usage: compile a statically linked musl binary
          staticShell = pkgs.mkShell {
            shellHook = ''
              ulimit -n 1024
              export RUSTFLAGS='-C target-feature=+crt-static'
              export CARGO_BUILD_TARGET='x86_64-unknown-linux-musl'
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
            '';
            buildInputs = [ fenixMusl ] ++ buildDeps;
          };

          # usage: link with mold
          moldShell = pkgs.mkShell {
            LD_LIBRARY_PATH = "${pkgs.zlib.out}/lib";
            buildInputs = with pkgs; [ zlib.out fd fenixStable ] ++ buildDeps;
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
              export RUSTFLAGS='-Clinker=${pkgs.clang}/bin/clang -Clink-arg=-fuse-ld=${pkgs.mold}/bin/mold'
            '';
          };

          # usage: setup for tokio with console
          #        with support for opentelemetry
          consoleShell = pkgs.mkShell {
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
            '';
            OTEL_BSP_MAX_EXPORT_BATCH_SIZE = 25;
            OTEL_BSP_MAX_QUEUE_SIZE = 32768;
            OTL_ENABLED = "true";
            TOKIO_CONSOLE_ENABLED = "true";
            RUSTFLAGS = "--cfg tokio_unstable";
            RUST_LOG = "tokio=trace,runtime=trace";
            LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib/";
            OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include/";
            OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib/";
            buildInputs = with pkgs;
              [ openssl.dev openssl.out tokio-console fenixStable ripgrep ]
              ++ buildDeps;
          };

          # usage: evaluate performance (llvm-cov + flamegraph)
          perfShell = pkgs.mkShell {
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
            '';
            buildInputs = with pkgs;
              [ cargo-flamegraph fd cargo-llvm-cov fenixStable ripgrep ]
              ++ buildDeps ++ lib.optionals stdenv.isLinux [
                heapstack_pkgs.heaptrack
                pkgs.valgrind
              ];
          };

          # usage: brings in debugging tools including:
          # - lldb: a debugger to be used with vscode
          debugShell = pkgs.mkShell {
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
            '';
            buildInputs = with pkgs; [ fenixStable lldb ] ++ buildDeps;
          };

          # run with `nix develop .#cudaShell`
          cudaShell = 
            let cudatoolkit = pkgsAllowUnfree.cudaPackages_12_3.cudatoolkit;
            in pkgs.mkShell {
            buildInputs = with pkgs; [ cmake cudatoolkit util-linux gcc11 fenixStable ] ++ buildDeps;
            shellHook = ''
              export CARGO_TARGET_DIR="$PWD/${CARGO_TARGET_DIR}";
              export PATH="${pkgs.gcc11}/bin:${cudatoolkit}/bin:${cudatoolkit}/nvvm/bin:$PATH"
              export LD_LIBRARY_PATH=${cudatoolkit}/lib
              export CUDA_PATH=${cudatoolkit}
              export CPATH="${cudatoolkit}/include"
              export LIBRARY_PATH="$LIBRARY_PATH:/lib"
              export CMAKE_CUDA_COMPILER=$CUDA_PATH/bin/nvcc
              export LIBCLANG_PATH=${pkgs.llvmPackages_15.libclang.lib}/lib
              export CFLAGS=""
            '';
          };

        };
        # TODO uncomment when fetching dependencies is unborked
        # packages = pkgsAndChecksAttrSet.packages;
        # checks = pkgsAndChecksAttrSet.checks;

        # defaultPackage = project.workspaceMembers.hotshot.build;
      });
}
