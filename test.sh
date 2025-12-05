#!/bin/bash

cargo run --release -- \
    --path "$1" \
    --codec hevc_videotoolbox \
    --always-reencode \
    --denoise \
    --buffer-count 5

    # --no-delete \