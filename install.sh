#!/bin/bash

RED="\e[1;31m"
BLUE="\e[1;36m"
GREEN="\e[1;33m"
WHITE="\e[1;37m"

download() {
    local url="$1"
    local output="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -fL "$url" -o "$output"
    elif command -v wget >/dev/null 2>&1; then
        wget -O "$output" "$url"
    else
        printf "%bError: neither curl nor wget is installed.%b\n" "$RED" "$WHITE" >&2
        return 1
    fi
}

