{ buildNpmPackage
, lib
, makeWrapper
, nodejs_24
}:

buildNpmPackage {
  pname = "npm-global-tools";
  version = "2026.06.19";

  src = builtins.path {
    path = ../tools/npm-globals;
    name = "npm-global-tools-src";
  };
  npmDepsFetcherVersion = 2;
  npmDepsHash = "sha256-zleFDEHaqA0MJtYvVlfTmQoozUqK78VO0YM/BF6fdyI=";

  dontNpmBuild = true;
  nativeBuildInputs = [ makeWrapper ];

  installPhase = ''
    runHook preInstall

    mkdir -p "$out/lib/npm-global-tools" "$out/bin"
    cp -R package.json package-lock.json node_modules "$out/lib/npm-global-tools/"

    for name in ni nci nr nup nd nlx na nun qmd pi; do
      bin="$out/lib/npm-global-tools/node_modules/.bin/$name"
      target="$(readlink "$bin")"

      if [ -n "$target" ]; then
        makeWrapper "$out/lib/npm-global-tools/node_modules/.bin/$target" "$out/bin/$name" \
          --prefix PATH : ${lib.makeBinPath [ nodejs_24 ]}
      else
        makeWrapper "$bin" "$out/bin/$name" \
          --prefix PATH : ${lib.makeBinPath [ nodejs_24 ]}
      fi
    done

    runHook postInstall
  '';
}
