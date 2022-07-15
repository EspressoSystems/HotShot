{ pkgs ? import <nixpkgs> {}, ... }:

pkgs.mkShell {
  buildInputs = [
    # trim this down
    pkgs.texlive.combined.scheme-full
  ];
}

