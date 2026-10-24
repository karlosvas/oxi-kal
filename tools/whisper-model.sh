#!/usr/bin/env bash
# Descarga el modelo de voz (Whisper small, cuantizado q8_0, ~265 MB) en
# apps/oxide-kcal/assets/whisper/, de donde lo toman el APK y la versión de escritorio.
#
#   tools/whisper-model.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
name="ggml-small-q8_0.bin"
sha256="49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f"
dest="$root/apps/oxide-kcal/assets/whisper/$name"

if [[ -f "$dest" ]] && echo "$sha256  $dest" | sha256sum -c --status; then
    echo "Modelo ya presente: $dest"
    exit 0
fi

mkdir -p "$(dirname "$dest")"
curl -fL --progress-bar -o "$dest.part" "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$name"
echo "$sha256  $dest.part" | sha256sum -c --status || { echo "Suma SHA-256 incorrecta" >&2; rm -f "$dest.part"; exit 1; }
mv "$dest.part" "$dest"
echo "Modelo: $dest"
