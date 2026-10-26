#!/usr/bin/env bash
set -euo pipefail

version="${1:?Использование: ./release.sh 1.1}"
if [[ ! "$version" =~ ^([0-9]+)\.([0-9]+)(\.([0-9]+))?$ ]]; then
    echo "Версия должна быть вида 1.1 или 1.1.1" >&2
    exit 1
fi
major=${BASH_REMATCH[1]}
minor=${BASH_REMATCH[2]}
patch=${BASH_REMATCH[4]:-0}
code=$((major * 10000 + minor * 100 + patch))

cd "$(dirname "$0")"

if [[ -n $(git status --porcelain) ]]; then
    echo "Есть незакоммиченные изменения — сначала закоммитьте их" >&2
    exit 1
fi
if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
    echo "Тег v$version уже есть" >&2
    exit 1
fi
for key in signing/keystore.properties signing/update.key; do
    if [[ ! -f "$key" ]]; then
        echo "Нет ключа подписи: $key" >&2
        exit 1
    fi
done

sed -i "0,/^version = .*/s//version = \"$major.$minor.$patch\"/" pc/Cargo.toml
sed -i "s/name=\"Crosbic.PhoneMedia\" version=\"[0-9.]*\"/name=\"Crosbic.PhoneMedia\" version=\"$major.$minor.$patch.0\"/" pc/assets/phone-media.manifest
sed -i "s/versionCode = [0-9]*/versionCode = $code/; s/versionName = \"[^\"]*\"/versionName = \"$version\"/" android/app/build.gradle.kts

(cd pc && cargo build --release)

export JAVA_HOME="${JAVA_HOME:-$LOCALAPPDATA/Programs/Android Studio/jbr}"
(cd android && ./gradlew assembleRelease --console=plain -q)

apk=android/app/build/outputs/apk/release/app-release.apk
sdk=$(sed -n 's/^sdk\.dir=//p' android/local.properties)
apksigner=$(ls "$sdk"/build-tools/*/apksigner.bat | sort -V | tail -1)
"$apksigner" verify "$apk"

out="dist/$version"
rm -rf "$out"
mkdir -p "$out"
cp pc/target/release/phone-media.exe "$out/PhoneMedia-$version.exe"
pc/target/release/sign.exe signing/update.key "$out/PhoneMedia-$version.exe"
cp "$apk" "$out/PhoneMedia-$version.apk"
(cd "$out" && sha256sum "PhoneMedia-$version.exe" "PhoneMedia-$version.apk" > SHA256SUMS)

git add pc/Cargo.toml pc/Cargo.lock pc/assets/phone-media.manifest android/app/build.gradle.kts
if ! git diff --cached --quiet; then
    git commit -q -m "Phone Media $version"
fi
git tag -a "v$version" -m "Phone Media $version"

echo "Релиз $version готов: $out"
ls -l "$out"
