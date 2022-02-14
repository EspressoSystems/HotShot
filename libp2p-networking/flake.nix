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
        fenixPackage = fenix.packages.${system}.stable.withComponents [ "cargo" "clippy" "rust-src" "rustc" "rustfmt" ];
        rustOverlay = final: prev:
          {
            inherit fenixPackage;
            rustc = fenixPackage;
            cargo = fenixPackage;
            rust-src = fenixPackage;
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

        shellHook = 
        # ''
        #   export ASYNC_STD_THREAD_COUNT=1
        # '' + 
        (pkgs.lib.optionals pkgs.stdenv.isDarwin '' 
          ulimit -n 9999999999
        '');
      in
      {
        packages.${crateName} = project.rootCrate.build;
        checks.${crateName} = project.rootCrate.build.override {
          runTests = true;
        };

        defaultPackage = self.packages.${system}.${crateName};

        devShell = pkgs.mkShell {
          inherit shellHook;
          # inputsFrom = builtins.attrValues self.packages.${system};
          buildInputs =
            with pkgs; [ cargo-audit nixpkgs-fmt git-chglog fenix.packages.${system}.rust-analyzer fenixPackage protobuf]
              ++ lib.optionals stdenv.isDarwin [ darwin.apple_sdk.frameworks.Security pkgs.libiconv darwin.apple_sdk.frameworks.SystemConfiguration recent_flamegraph fd ];
        };

      });
}
