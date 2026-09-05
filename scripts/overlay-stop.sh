#!/usr/bin/env bash
# Stop the overlay on Windows.
taskkill.exe /IM featherstorm.exe /F 2>&1 | tr -d '\r' | tail -1
