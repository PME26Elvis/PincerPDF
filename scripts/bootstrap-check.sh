#!/usr/bin/env bash
set -uo pipefail

required=(bash git python3 rustc cargo rustup node npm qpdf mutool xvfb-run chromium ffmpeg convert)
missing=()

printf 'PincerPDF bootstrap check\n'
printf 'kernel=%s\n' "$(uname -srmo)"
printf 'arch=%s\n' "$(uname -m)"

for command in "${required[@]}"; do
  if path="$(command -v "$command" 2>/dev/null)"; then
    printf 'tool.%s=present:%s\n' "$command" "$path"
  else
    printf 'tool.%s=missing\n' "$command"
    missing+=("$command")
  fi
done

if ((${#missing[@]} > 0)); then
  printf 'status=incomplete\n'
  printf 'missing=%s\n' "$(IFS=,; echo "${missing[*]}")"
  exit 2
fi

printf 'rustc=%s\n' "$(rustc --version)"
printf 'cargo=%s\n' "$(cargo --version)"
printf 'node=%s\n' "$(node --version)"
printf 'qpdf=%s\n' "$(qpdf --version | head -n 1)"
printf 'status=ready\n'
