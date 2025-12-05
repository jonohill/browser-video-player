#!/bin/bash

cargo run --release -- \
    --path "$1" \
    --codec hevc_videotoolbox \
    --always-reencode \
    --denoise \
    --buffer-count 1

    # --no-delete \