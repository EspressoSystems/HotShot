{
  description = "Justin Restivo Resume";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/master";
  inputs.utils.url = "github:numtide/flake-utils";
  inputs.flake-compat = {
    url = "github:edolstra/flake-compat";
    flake = false;
  };

  outputs = { self, nixpkgs, utils, ...}:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        texlive = pkgs.texlive.combine {
          inherit (pkgs.texlive)
            scheme-small multirow xstring totpages environ ncctools comment
            hyperxmp ifmtarg preprint latexmk libertine inconsolata shade newtx
            algorithm2e ifoddpage relsize geometry parskip graphics latex amsmath mathtools pgf hyperref ec
            ;
        };
        hotshot-paper = pkgs.stdenv.mkDerivation {
          name = "hotshot-paper";
          src = ./pandoc-based-spec/src;
          buildInputs = [
            texlive
            pkgs.glibcLocales
            pkgs.pandoc
            pkgs.fontconfig
            pkgs.haskellPackages.pandoc-crossref
          ];
          FONTCONFIG_FILE =
            pkgs.makeFontsConf { fontDirectories = [ pkgs.lmodern ]; };
          buildPhase = "make";
          installPhase = "cp *.pdf $out";
        };
        hotshot-analysis = pkgs.stdenv.mkDerivation {
          name = "hotshot-analysis";
          src = ./hotshot-analysis;
          buildInputs = [
            texlive
            pkgs.fontconfig
          ];
          FONTCONFIG_FILE =
            pkgs.makeFontsConf { fontDirectories = [ pkgs.lmodern ]; };
          buildPhase = ''
            mkdir -p .cache/texmf-var
            env TEXMFHOME=.cache TEXMFVAR=.cache/texmf-var latexmk -interaction=nonstopmode -pdf -lualatex analysis.tex
          '';
          installPhase = "cp *.pdf $out";
        };
      in {
        defaultPackage = hotshot-paper;
        packages = {
          inherit hotshot-analysis hotshot-paper;
        };
        devShell = pkgs.mkShell {
          buildInputs = [
            texlive
            pkgs.glibcLocales
            pkgs.pandoc
            pkgs.fontconfig
            pkgs.haskellPackages.pandoc-crossref
          ];
        };
      });
}
