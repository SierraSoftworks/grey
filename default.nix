let
  pkgs = import <nixpkgs> {};
in with pkgs; {
  grey = import ./grey.nix { inherit lib rustPlatform; };
}
