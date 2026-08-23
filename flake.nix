{
  description = "plexus";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    inputs@{ crane, flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem =
        { pkgs, ... }:
        let
          craneLib = crane.mkLib pkgs;

          commonArgs = {
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
          };

          artifacts = craneLib.buildDepsOnly commonArgs;

          plexus = craneLib.buildPackage (
            commonArgs
            // {
              cargoArtifacts = artifacts;
              meta.mainProgram = "plexus";
            }
          );
        in
        {
          packages = {
            default = plexus;
            inherit plexus;
          };

          devShells.default = craneLib.devShell {
            inputsFrom = [ plexus ];
            packages = with pkgs; [
              rust-analyzer

              perf
              hyperfine
              glow
              lldb
              gdb
              gnumake
              gawk
              python3
            ];
          };

          checks.default = plexus;
        };
    };
}
