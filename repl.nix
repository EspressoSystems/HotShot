# usage: nix repl repl.nix
let
  flake = builtins.getFlake (toString ./.);
in
{
  inherit flake;
  self = flake.inputs.self;
  pkgs = flake.inputs.nixpkgs.legacyPackages;
}
