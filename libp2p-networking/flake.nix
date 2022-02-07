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

        shellHook = '''' + (pkgs.lib.optionals pkgs.stdenv.isDarwin '' ulimit -n 9999999999'');
      in
      {
        packages.${crateName} = project.rootCrate.build;
        checks.${crateName} = project.rootCrate.build.override {
          runTests = true;
        };

        defaultPackage = self.packages.${system}.${crateName};

        devShell = pkgs.mkShell {
          inherit shellHook;
          inputsFrom = builtins.attrValues self.packages.${system};
          buildInputs =
            with pkgs; [ cargo-audit nixpkgs-fmt git-chglog fenix.packages.${system}.rust-analyzer fenixPackage protobuf]
              ++ lib.optionals stdenv.isDarwin [ darwin.apple_sdk.frameworks.Security pkgs.libiconv darwin.apple_sdk.frameworks.SystemConfiguration ];
        };
      });
}
