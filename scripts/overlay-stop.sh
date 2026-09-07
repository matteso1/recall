#!/usr/bin/env bash
# Stop the overlay on Windows.
taskkill.exe /IM recall.exe /F 2>&1 | tr -d '\r' | tail -1
# an overlay built before the rename
taskkill.exe /IM featherstorm.exe /F >/dev/null 2>&1 || true
