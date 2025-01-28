#!/bin/bash
backup_dir=~/solana_bot_backup/$(date +%Y%m%d_%H%M%S)
mkdir -p $backup_dir
cp -r config src Cargo.toml README.md $backup_dir
echo "Backup created at $backup_dir"