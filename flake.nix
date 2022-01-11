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
        fenixPackage = fenix.packages.${system}.stable.withComponents [ "cargo" "clippy" "rust-src" "rustc" "rustfmt"];
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
            rootFeatures = [ "demo" "docs" "blake3" "threshold_crypto/use-insecure-test-only-mock-crypto" ] ;
            defaultCrateOverrides = pkgs.defaultCrateOverrides // {
              # Crate dependency overrides go here
            };
          };

      in
      {
        packages.${crateName} = project.rootCrate.build;
        checks.${crateName} = project.rootCrate.build.override {
          runTests = true;
        };

        defaultPackage = self.packages.${system}.${crateName};

        devShell = pkgs.mkShell {
          inputsFrom = builtins.attrValues self.packages.${system};
          buildInputs =
            with pkgs; [ cargo-audit nixpkgs-fmt git-chglog fenix.packages.${system}.rust-analyzer fenixPackage ];
        };
      });
}
