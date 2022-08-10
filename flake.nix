{
  description = "HotShot consensus library";

  nixConfig = {
    extra-substituters = ["https://espresso-systems-private.cachix.org"];
    extra-trusted-public-keys = ["espresso-systems-private.cachix.org-1:LHYk03zKQCeZ4dvg3NctyCq88e44oBZVug5LpYKjPRI="];
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
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-compat, utils, crate2nix, fenix, rust-overlay }:
    utils.lib.eachDefaultSystem (system:
      let
        # rustNightly = rust-overlay.packages.${system}."rust-nightly_2022-07-17";
        rustNightly = rust-overlay.packages.${system}."rust-nightly_2022-07-17".override {
          extensions = [ "rustfmt" "clippy" "llvm-tools-preview" "rust-src" ];
        };
        # needed for compiling static binary
        fenixMusl = with fenix.packages.${system}; combine [ (stable.withComponents [ "cargo" "clippy" "rustc" "rustfmt" ]) targets.x86_64-unknown-linux-musl.stable.rust-std ];

        rustOverlay = final: prev:
          {
            rustc = rustNightly;
            cargo = rustNightly;
            rust-src = rustNightly;
          };

        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            rustOverlay
          ];
        };

        heapstack_pkgs = import nixpkgs {
          inherit system;
        };

        cargo-llvm-cov = pkgs.rustPlatform.buildRustPackage rec {
          pname = "cargo-llvm-cov";
          version = "0.3.0";

          doCheck = false;

          buildInputs = [ pkgs.libllvm ];

          src = builtins.fetchTarball {
            url = "https://crates.io/api/v1/crates/${pname}/${version}/download";
            sha256 = "sha256:0iswa2cdaf2123vfc42yj9l8jx53k5jm2y51d4xqc1672hi4620l";
          };

          cargoSha256 = "sha256-RzIkW/eytU8ZdZ18x0sGriJ2xvjVW+8hB85In12dXMg=";
          meta = with pkgs.lib; {
            description = "Cargo llvm cov generates code coverage via llvm.";
            homepage = "https://github.com/taiki-e/cargo-llvm-cov";

            license = with licenses; [ mit asl20 ];
          };
        };


        # DON'T FORGET TO PUT YOUR PACKAGE NAME HERE, REMOVING `throw`
        crateName = "hotshot";

        inherit (import "${crate2nix}/tools.nix" { inherit pkgs; })
          generatedCargoNix;

        project = import
          (generatedCargoNix {
            name = crateName;
            src = ./.;
          })
          {
            inherit pkgs;
            rootFeatures = [ "demo" "docs" "blake3" ];
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
            with pkgs; [ rustNightly ] ++ buildDeps;
        };


        devShells = {
          docsShell = pkgs.mkShell {
            buildInputs = [
              pkgs.glibcLocales
              pkgs.pandoc
              pkgs.fontconfig
              pkgs.haskellPackages.pandoc-crossref
            ];
          };
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

          moldShell = pkgs.mkShell {
            LD_LIBRARY_PATH="${pkgs.zlib.out}/lib";
            buildInputs = with pkgs; [ zlib.out fd fenixNightly ] ++ buildDeps;
            shellHook = ''
              export RUSTFLAGS='-Clinker=${pkgs.clang}/bin/clang -Clink-arg=-fuse-ld=${pkgs.mold}/bin/mold'
            '';
          };

          # usage: evaluate performance (llvm-cov + flamegraph)
          perfShell = pkgs.mkShell {
             buildInputs = with pkgs; [ cargo-flamegraph fd cargo-llvm-cov rustNightly ripgrep ] ++ buildDeps ++ lib.optionals stdenv.isLinux [ heapstack_pkgs.heaptrack pkgs.valgrind ];
          };

          # usage: brings in debugging tools including:
          # - lldb: a debugger to be used with vscode
          debugShell = pkgs.mkShell {
            buildInputs =
              with pkgs; [ rustNightly lldb ] ++ buildDeps;
          };

        };
        # TODO uncomment when fetching dependencies is unborked
        # packages = pkgsAndChecksAttrSet.packages;
        # checks = pkgsAndChecksAttrSet.checks;

        # defaultPackage = project.workspaceMembers.hotshot.build;
      }
    );
}
