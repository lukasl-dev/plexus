{
  description = "plexus";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    zig.url = "github:silversquirl/zig-flake";
    zls = {
      url = "github:zigtools/zls";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        zig-flake.follows = "zig";
      };
    };
    zon2nix.url = "github:nix-community/zon2nix";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem =
        {
          inputs',
          pkgs,
          self',
          system,
          ...
        }:
        let
          target = builtins.replaceStrings [ "darwin" ] [ "macos" ] system;

          cleanSource = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: _type:
              let
                rel = pkgs.lib.removePrefix (toString ./. + "/") (toString path);
              in
              rel == "build.zig"
              || rel == "build.zig.zon"
              || rel == "LICENSE"
              || rel == "README.md"
              || rel == "src"
              || pkgs.lib.hasPrefix "src/" rel;
          };

          zig = inputs'.zig.packages.nightly;

          mkDerivation =
            optimize:
            pkgs.stdenv.mkDerivation {
              name = "plexus";
              version = "master";
              meta.mainProgram = "plexus";
              src = cleanSource;
              nativeBuildInputs = [
                zig
              ];
              dontInstall = true;
              doCheck = true;
              configurePhase = ''
                export ZIG_GLOBAL_CACHE_DIR=$TEMP/.cache
              '';
              buildPhase = ''
                PACKAGE_DIR=${pkgs.callPackage ./deps.nix { }}
                zig build install \
                  --system $PACKAGE_DIR \
                  -Dtarget=${target} \
                  -Doptimize=${optimize} \
                  --color off \
                  --prefix $out
              '';
              checkPhase = ''
                zig build test \
                  --system $PACKAGE_DIR \
                  -Dtarget=${target} \
                  --color off
              '';
            };

          zon2nix = pkgs.writeShellApplication {
            name = "zon2nix";
            text = ''
              ${inputs'.zon2nix.packages.default}/bin/zon2nix > deps.nix
            '';
          };
        in
        {
          devShells.default = pkgs.mkShell {
            buildInputs = [
              zig
              inputs'.zls.packages.zls
              zon2nix
            ]
            ++ (with pkgs; [
              perf
              hyperfine
              glow
              lldb
              gdb
              gnumake
              gawk
              python3
            ]);
          };

          packages = rec {
            default = plexus;

            plexus = plexus-release-safe;
            plexus-debug = mkDerivation "Debug";
            plexus-release-safe = mkDerivation "ReleaseSafe";
            plexus-release-fast = mkDerivation "ReleaseFast";
          };

          checks.default = self'.packages.default;
        };
    };
}
